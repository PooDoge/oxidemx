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
// NOTE: this blanket impl makes the trait non-object-safe and prevents external
// crates from adding their own bounded impls — if cross-crate extension is
// needed later, convert to a manual per-type impl or move the trait to a lib crate.
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

    // ---- region components ------------------------------------------------
    //
    // Each calls `use_region_nav` internally — the seam under test.
    // `LeftRegion` renders a "nav-btn" rect; clicking it calls
    // `RegionNav::navigate(LeftPage::B)` through the seam.
    //
    // Window layout (600 × 400): left region occupies x=0..300, center x=300..600.
    // The nav button fills the left region so any click inside x<300 triggers it.

    fn left_region() -> impl IntoElement {
        let nav = use_region_nav(LeftPage::A);
        let mut nav_for_click = nav.clone();

        rect()
            .width(Size::flex(1.0))
            .height(Size::fill())
            // Navigate button — clicking anywhere in the left region calls navigate().
            .child(
                rect()
                    .expanded()
                    .on_mouse_up(move |_| nav_for_click.navigate(LeftPage::B))
                    .child(match nav.current() {
                        LeftPage::A => label().text("left-A").into_element(),
                        LeftPage::B => label().text("left-B").into_element(),
                    }),
            )
    }

    fn center_region() -> impl IntoElement {
        let nav = use_region_nav(CenterPage::X);

        rect()
            .width(Size::flex(1.0))
            .height(Size::fill())
            .child(match nav.current() {
                CenterPage::X => label().text("center-X").into_element(),
                CenterPage::Y => label().text("center-Y").into_element(),
            })
    }

    fn two_region_app() -> impl IntoElement {
        rect()
            .expanded()
            .direction(Direction::Horizontal)
            .child(left_region().into_element())
            .child(center_region().into_element())
    }

    // ---- invariant test ---------------------------------------------------
    //
    // Drives navigation THROUGH the seam: click_cursor fires on_mouse_up inside
    // left_region, which calls RegionNav::navigate(LeftPage::B).  A broken seam
    // (e.g. shared static State) would change center_region's output too.

    /// Region A navigates A→B via `RegionNav::navigate`; region B must remain on its initial page.
    #[test]
    fn region_nav_is_independent() {
        let mut runner = launch_test(two_region_app);

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

        // Navigate left region A → B by clicking inside the left region.
        // The on_mouse_up handler calls nav.navigate(LeftPage::B) through the seam.
        // Left region occupies x=0..250, y=0..500 (500×500 default from launch_test).
        runner.click_cursor((125., 250.));

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
