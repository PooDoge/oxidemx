//! Left region: a Conversations page (project + conversation list) plus a
//! placeholder second page, to prove the sidebar navigates without touching
//! the center region.
use freya::prelude::*;
use oxide_ui::components::{CollapsiblePanel, ListItem, RailButton};

use crate::nav::use_region_nav;
use crate::state::AppState;

#[derive(Clone, PartialEq)]
pub enum SidebarPage {
    Conversations,
    Placeholder,
}

#[derive(PartialEq, Clone)]
pub struct Sidebar {
    pub state: AppState,
    pub collapsed: bool,
}

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let nav = use_region_nav(SidebarPage::Conversations);
        let collapsed = use_state(|| false);
        let state = self.state.clone();
        let mut nav_for_click = nav.clone();

        // Each closure needs its own copy of the State handle (State is Copy).
        let mut collapsed_for_collapse = collapsed;
        let mut collapsed_for_expand = collapsed;

        let full = match nav.current() {
            SidebarPage::Conversations => {
                let convs = state.conversations.read().clone();
                let mut col = rect()
                    .direction(Direction::Vertical)
                    .spacing(4.0)
                    .width(Size::fill())
                    .height(Size::fill());
                // Nav button to switch to Placeholder page (proves independent nav).
                col = col.child(
                    rect()
                        .padding(Gaps::new_all(8.0))
                        .on_mouse_up(move |_| nav_for_click.navigate(SidebarPage::Placeholder))
                        .child(label().text("≡ Conversations").color(Color::WHITE)),
                );
                for c in convs {
                    let st = state.clone();
                    let id = c.id.clone();
                    col = col.child(
                        ListItem::new(c.title.clone())
                            .on_press(move |_| st.open_conversation(id.clone())),
                    );
                }
                // Spacer to push the collapse button to the bottom.
                col = col.child(rect().width(Size::fill()).height(Size::flex(1.0)));
                // Collapse toggle: press "«" to collapse.
                col = col.child(
                    RailButton::new("«".into())
                        .on_press(move |_: Event<PressEventData>| collapsed_for_collapse.set(true)),
                );
                col.into_element()
            }
            SidebarPage::Placeholder => {
                rect()
                    .direction(Direction::Vertical)
                    .child(label().text("Sidebar placeholder page").color(Color::WHITE))
                    .into_element()
            }
        };

        // Rail: press "»" to expand.
        let rail = RailButton::new("»".into())
            .on_press(move |_: Event<PressEventData>| collapsed_for_expand.set(false));

        CollapsiblePanel::new()
            .collapsed(*collapsed.read())
            .full(full)
            .rail(rail)
    }
}
