//! Left region: project header + styled conversation list + collapse rail.
//!
//! The placeholder-page nav demo from 2a has been removed. The per-region-nav
//! independence invariant is covered by `nav::tests::region_nav_is_independent`.
use freya::animation::*;
use freya::prelude::*;
use oxide_ui::{
    Theme,
    components::{CollapsiblePanel, ListItem, RailButton, SidebarHeader, StatusDot},
    tokens::{SIDEBAR_FULL_W, SIDEBAR_RAIL_W},
};

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct Sidebar {
    pub state: AppState,
}

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let collapsed = self.state.sidebar_collapsed;
        let mut c_collapse = collapsed;
        let mut c_expand = collapsed;
        let state = self.state.clone();
        let th = Theme::default();

        let is_collapsed = *collapsed.read();
        let width_anim = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            let w = AnimNum::new(SIDEBAR_FULL_W, SIDEBAR_RAIL_W)
                .time(180)
                .ease(Ease::Out)
                .function(Function::Quart);
            // Read `collapsed` (State<bool>) inside the closure so the Effect
            // subscribes to it and re-fires only on collapse-toggle.
            if *collapsed.read() { w } else { w.into_reversed() }
        });
        let anim_w = width_anim.get().value();

        let convs = state.conversations.read().clone();
        let active = state.active.read().clone();

        // Full panel: header + scrollable conversation list + collapse button.
        let mut col = rect()
            .direction(Direction::Vertical)
            .content(Content::Flex)
            .width(Size::fill())
            .height(Size::fill())
            .background(th.panel())
            .child({
                let st = state.clone();
                let projects: Vec<(String, String)> = st
                    .projects
                    .read()
                    .iter()
                    .map(|p| (p.id.0.clone(), p.name.clone()))
                    .collect();
                let current_id = st
                    .current_project
                    .read()
                    .as_ref()
                    .map(|p| p.0.clone())
                    .unwrap_or_default();
                let on_st = state.clone();
                SidebarHeader::new(projects, current_id)
                    .on_select(move |id: String| on_st.open_project(id.into()))
                    .theme(th)
            });

        let mut list = rect()
            .direction(Direction::Vertical)
            .spacing(3.)
            .width(Size::fill());
        for c in convs {
            let st = state.clone();
            let id = c.id.clone();
            let sel = active.as_ref() == Some(&c.id);
            list = list.child(
                ListItem::new(c.title.clone())
                    .selected(sel)
                    .state("idle".into())
                    .worktree(c.worktree.as_ref().map(|w| w.branch.clone()))
                    .theme(th)
                    .on_press(move |_| st.open_conversation(id.clone())),
            );
        }

        col = col.child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.0))
                .padding(Gaps::new(0., 8., 0., 8.))
                .child(ScrollView::new().show_scrollbar(false).child(list)),
        );

        col = col.child(
            rect()
                .padding(Gaps::new_all(8.))
                .child(
                    RailButton::new("«".into())
                        .on_press(move |_: Event<PressEventData>| c_collapse.set(true)),
                ),
        );

        // Collapsed rail: project dot, one icon per conversation, expand button.
        let convs_rail = state.conversations.read().clone();
        let active_rail = state.active.read().clone();
        let mut rail = rect()
            .direction(Direction::Vertical)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .height(Size::fill())
            // project dot at top — press expands.
            .child(
                rect()
                    .width(Size::px(10.)).height(Size::px(10.))
                    .corner_radius(CornerRadius::new_all(5.))
                    .background(th.accent())
                    .on_press({
                        let mut e = c_expand;
                        move |_: Event<PressEventData>| e.set(false)
                    }),
            );
        for c in convs_rail {
            let st = state.clone();
            let id = c.id.clone();
            let sel = active_rail.as_ref() == Some(&c.id);
            let mut e = c_expand;
            rail = rail.child(
                rect()
                    .width(Size::px(28.)).height(Size::px(28.))
                    .corner_radius(CornerRadius::new_all(8.))
                    .center()
                    .background(if sel { th.surface_hi() } else { th.surface() })
                    .on_press(move |_: Event<PressEventData>| {
                        st.open_conversation(id.clone());
                        e.set(false);
                    })
                    .child(StatusDot::new(true)),
            );
        }
        let rail = rail.child(
            rect()
                .height(Size::flex(1.0))
                .cross_align(Alignment::Center)
                .child(
                    RailButton::new("»".into())
                        .on_press(move |_: Event<PressEventData>| c_expand.set(false)),
                ),
        );

        CollapsiblePanel::new()
            .collapsed(is_collapsed)
            .override_width(Some(anim_w))
            .full(col.into_element())
            .rail(rail)
    }
}
