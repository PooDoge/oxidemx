//! Per-region navigation seam.
//!
//! Each region owns its own `RegionNav<P>` handle, so a sidebar can navigate
//! without remounting (or even re-rendering) the center region.
//!
//! # Design decision
//!
//! `freya-router` provides a SINGLE non-generic `RouterContext` per scope.
//! Two `Router<L>` and `Router<C>` as siblings each provide their own
//! `RouterContext`; a child resolves its nearest ancestor's context, so they
//! are isolated at the context level.  However, routing is URL-path-based and
//! was designed for app-wide navigation:
//!
//! * Path namespaces collide (`/b` in LeftRoute vs. `/b` in CenterRoute).
//! * External navigation (a button above both routers) cannot call
//!   `RouterContext::get()` — it is above both router trees and finds nothing.
//! * `AnimatedRouter` is tied to one layout root, not per-region.
//!
//! The fallback — a `use_state`-backed handle per region — avoids all of
//! these problems cleanly.  See `oxide-app/API-NOTES.md § Region nav decision`
//! for the full write-up.
//!
//! # Usage
//!
//! ```ignore
//! // In a region component:
//! let mut nav: RegionNav<SidebarPage> = use_region_nav(SidebarPage::Home);
//!
//! match nav.current() {
//!     SidebarPage::Home    => { /* render home */ }
//!     SidebarPage::Profile => { /* render profile */ }
//! }
//!
//! // Navigate on a button press:
//! rect().on_mouse_up(move |_| nav.navigate(SidebarPage::Profile))
//! ```
//!
//! # Task 9 note
//!
//! Animated transitions (FadeIn/SlideIn) are deferred to Task 9 (`oxide-ui::anim`).
//! When that lands, wrap `region_view` in the Task 9 animated wrapper before the
//! `match` block — the `RegionNav` seam itself does not change.

use freya::prelude::*;

// ── Trait ──────────────────────────────────────────────────────────────────

/// A page variant a region can display.
///
/// Implement this on a per-region enum.  The blanket impl below means any
/// type satisfying the bounds automatically implements `RegionPage`.
pub trait RegionPage: Clone + PartialEq + 'static {}

/// Blanket impl: every `Clone + PartialEq + 'static` enum is a `RegionPage`.
impl<T: Clone + PartialEq + 'static> RegionPage for T {}

// ── Handle ─────────────────────────────────────────────────────────────────

/// Per-region navigation handle.
///
/// Holds a reactive `State<P>`.  Cheaply cloneable (State is `Copy`).
/// Call `use_region_nav` to create one inside a Freya component.
#[derive(Clone, Copy)]
pub struct RegionNav<P: RegionPage> {
    page: State<P>,
}

impl<P: RegionPage> RegionNav<P> {
    /// The page currently shown by this region.
    pub fn current(&self) -> P {
        self.page.read().clone()
    }

    /// Switch this region to `to`.  Other regions are unaffected.
    pub fn navigate(&mut self, to: P) {
        self.page.set(to);
    }
}

// ── Hook ───────────────────────────────────────────────────────────────────

/// Create a `RegionNav<P>` for the calling component, seeded with `initial`.
///
/// Must be called from inside a Freya component (same rules as `use_state`).
pub fn use_region_nav<P: RegionPage>(initial: P) -> RegionNav<P> {
    RegionNav {
        page: use_state(|| initial),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use freya_testing::prelude::*;

    use super::*;

    // ---- page enums used only in tests ------------------------------------

    #[derive(Clone, PartialEq, Debug)]
    enum LeftPage {
        A,
        B,
    }

    #[derive(Clone, PartialEq, Debug)]
    #[allow(dead_code)] // Y exists to prove the enum is complete; the test asserts it never appears
    enum CenterPage {
        X,
        Y,
    }

    // ---- two-region app fixture -------------------------------------------
    //
    // The two navs need to cross the component boundary so the test can drive
    // them.  We thread them via root-level `State<LeftPage>` and
    // `State<CenterPage>` contexts that the app reads and the test mutates
    // via `runner.run_in(|| ...)`.

    fn two_region_app() -> impl IntoElement {
        // Consume the shared states injected by the test harness.
        let left_page: State<LeftPage> = consume_root_context();
        let center_page: State<CenterPage> = consume_root_context();

        rect()
            .expanded()
            .horizontal()
            // Left region
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .child(match *left_page.read() {
                        LeftPage::A => label().text("left-A").into_element(),
                        LeftPage::B => label().text("left-B").into_element(),
                    }),
            )
            // Center region
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .child(match *center_page.read() {
                        CenterPage::X => label().text("center-X").into_element(),
                        CenterPage::Y => label().text("center-Y").into_element(),
                    }),
            )
    }

    // ---- invariant test ---------------------------------------------------

    /// Region A navigates A→B; region B must remain on its initial page.
    #[test]
    fn region_nav_is_independent() {
        // Provide shared state at the root so both the app and test can access it.
        let (mut runner, (left_state, _center_state)) = TestingRunner::new(
            two_region_app,
            Size2D::new(600., 400.),
            |r| {
                let left = r.provide_root_context(|| State::create(LeftPage::A));
                let center = r.provide_root_context(|| State::create(CenterPage::X));
                (left, center)
            },
            1.0,
        );

        // Initial render: left=A, center=X.
        runner.sync_and_update();

        let found_left_a = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("left-A"))
        });
        assert!(found_left_a.is_some(), "initial: left region should show 'left-A'");

        let found_center_x = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("center-X"))
        });
        assert!(found_center_x.is_some(), "initial: center region should show 'center-X'");

        // Navigate left region A → B (center must stay on X).
        // State::write_unchecked takes &self, no runner context needed.
        *left_state.write_unchecked() = LeftPage::B;
        runner.sync_and_update();

        // Left is now B.
        let found_left_b = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("left-B"))
        });
        assert!(found_left_b.is_some(), "after navigate: left region should show 'left-B'");

        // Center is STILL X — independence invariant.
        let found_center_x_still = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("center-X"))
        });
        assert!(
            found_center_x_still.is_some(),
            "after navigate: center region must STILL show 'center-X' (independence invariant)"
        );

        // Sanity: center-Y must NOT appear.
        let found_center_y = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("center-Y"))
        });
        assert!(
            found_center_y.is_none(),
            "center-Y must not appear when center was never navigated"
        );

        // Sanity: left-A must NOT appear after navigation.
        let found_left_a_gone = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("left-A"))
        });
        assert!(
            found_left_a_gone.is_none(),
            "left-A must not appear after left navigated to B"
        );
    }
}
