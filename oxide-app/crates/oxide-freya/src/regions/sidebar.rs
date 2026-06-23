//! Left region: project header + styled conversation list + collapse rail.
//!
//! The placeholder-page nav demo from 2a has been removed. The per-region-nav
//! independence invariant is covered by `nav::tests::region_nav_is_independent`.
use freya::prelude::*;
use oxide_ui::{
    Theme,
    components::{CollapsiblePanel, ListItem, RailButton, SidebarHeader},
};

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct Sidebar {
    pub state: AppState,
    pub collapsed: bool,
}

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let collapsed = use_state(|| false);
        let mut c_collapse = collapsed;
        let mut c_expand = collapsed;
        let state = self.state.clone();
        let th = Theme::default();

        let convs = state.conversations.read().clone();
        let active = state.active.read().clone();

        // Full panel: header + scrollable conversation list + collapse button.
        let mut col = rect()
            .direction(Direction::Vertical)
            .content(Content::Flex)
            .width(Size::fill())
            .height(Size::fill())
            .background(th.panel())
            .child(SidebarHeader::new("oxidemx-phase1".into()).theme(th));

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
                .child(ScrollView::new().child(list)),
        );

        col = col.child(
            rect()
                .padding(Gaps::new_all(8.))
                .child(
                    RailButton::new("«".into())
                        .on_press(move |_: Event<PressEventData>| c_collapse.set(true)),
                ),
        );

        // Rail: just the expand button.
        let rail = RailButton::new("»".into())
            .on_press(move |_: Event<PressEventData>| c_expand.set(false));

        CollapsiblePanel::new()
            .collapsed(*collapsed.read())
            .full(col.into_element())
            .rail(rail)
    }
}
