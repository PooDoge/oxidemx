//! Left region: a Conversations page (project + conversation list) plus a
//! placeholder second page, to prove the sidebar navigates without touching
//! the center region.
use freya::prelude::*;
use oxide_ui::components::{CollapsiblePanel, ListItem};

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
        let state = self.state.clone();
        let mut nav_for_click = nav.clone();

        let full = match nav.current() {
            SidebarPage::Conversations => {
                let convs = state.conversations.read().clone();
                let mut col = rect().direction(Direction::Vertical).spacing(4.0);
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
                col.into_element()
            }
            SidebarPage::Placeholder => {
                rect()
                    .direction(Direction::Vertical)
                    .child(label().text("Sidebar placeholder page").color(Color::WHITE))
                    .into_element()
            }
        };

        CollapsiblePanel::new()
            .collapsed(self.collapsed)
            .full(full)
            .rail(label().text("≡"))
    }
}
