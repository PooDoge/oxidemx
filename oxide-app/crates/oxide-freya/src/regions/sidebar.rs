//! Left region: project header + styled conversation list + collapse rail.
//!
//! The placeholder-page nav demo from 2a has been removed. The per-region-nav
//! independence invariant is covered by `nav::tests::region_nav_is_independent`.
use freya::prelude::*;
use freya_icons::lucide;
use oxide_ui::{
    Theme,
    components::{
        AttachedPosition, ListItem, MenuRow, MenuSurface, OxideTooltip, Placement, Popover,
        RailButton, SidebarHeader, StatusDot, TooltipGroup,
    },
};

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct Sidebar {
    pub state: AppState,
    /// Effective collapsed value (user signal OR compact size class), from `shell()`.
    pub collapsed: bool,
}

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let user_collapsed = self.state.sidebar_collapsed; // buttons write this
        let mut c_collapse = user_collapsed;
        let mut c_expand = user_collapsed;
        let state = self.state.clone();
        let th = Theme::default();

        let is_collapsed = self.collapsed; // EFFECTIVE (render)

        // Project dropdown open state — unconditional hook (collapsed rail uses it).
        let proj_open = use_state(|| false);

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
                    .on_new({
                        let st = state.clone();
                        move |_| st.create_conversation()
                    })
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

        // Collapsed rail: project dropdown, "+" create button, one icon per
        // conversation, expand button.
        let convs_rail = state.conversations.read().clone();
        let active_rail = state.active.read().clone();

        // ── Project dropdown button ───────────────────────────────────────────
        let projects = state.projects.read().clone();
        let current = state.current_project.read().clone();

        let proj_btn: Element = rect()
            .width(Size::px(28.)).height(Size::px(28.))
            .corner_radius(CornerRadius::new_all(8.))
            .center()
            .background(th.surface())
            .a11y_role(AccessibilityRole::Button)
            .a11y_alt("Switch project")
            .on_press({
                let mut p = proj_open;
                move |_: Event<PressEventData>| p.toggle()
            })
            .child(
                svg(lucide::chevrons_up_down())
                    .width(Size::px(16.))
                    .height(Size::px(16.))
                    .color(th.text()),
            )
            .into_element();

        let mut proj_rows = rect().direction(Direction::Vertical);
        for p in projects {
            let st = state.clone();
            let pid = p.id.clone();
            let is_selected = current.as_ref() == Some(&p.id);
            proj_rows = proj_rows.child(
                MenuRow::new(th)
                    .title(p.name.clone())
                    .selected(is_selected)
                    .on_press({
                        let mut po = proj_open;
                        move |()| {
                            st.open_project(pid.clone());
                            po.set(false);
                        }
                    }),
            );
        }

        let proj_menu_surface = MenuSurface::new(th)
            .min_w(180.)
            .on_close({
                let mut p = proj_open;
                move |()| p.set(false)
            })
            .child(proj_rows);

        let proj_popover = OxideTooltip::text("Switch project")
            .placement(AttachedPosition::Right)
            .offset(6.)
            .child(
                Popover::new(proj_btn)
                    .open(*proj_open.read())
                    .placement(Placement::Below)
                    .content(proj_menu_surface)
                    .into_element(),
            );

        // ── "+" create-conversation button ────────────────────────────────────
        let new_btn: Element = rect()
            .width(Size::px(28.)).height(Size::px(28.))
            .corner_radius(CornerRadius::new_all(8.))
            .center()
            .background(th.accent())
            .a11y_role(AccessibilityRole::Button)
            .a11y_alt("New conversation")
            .on_press({
                let st = state.clone();
                move |_: Event<PressEventData>| st.create_conversation()
            })
            .child(
                svg(lucide::plus())
                    .width(Size::px(16.))
                    .height(Size::px(16.))
                    .color(th.bg_deep()),
            )
            .into_element();

        let new_btn_tooltip = OxideTooltip::text("New conversation")
            .placement(AttachedPosition::Right)
            .offset(6.)
            .child(new_btn);

        // ── Rail assembly ─────────────────────────────────────────────────────
        let mut rail = rect()
            .direction(Direction::Vertical)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .height(Size::fill())
            .child(proj_popover)
            .child(new_btn_tooltip);

        for c in convs_rail {
            let st = state.clone();
            let id = c.id.clone();
            let sel = active_rail.as_ref() == Some(&c.id);
            let detail = rect()
                .direction(Direction::Vertical)
                .spacing(3.)
                .child(label().max_lines(1).text(c.title.clone()).font_size(12.5).color(th.text()))
                .child(
                    label()
                        .max_lines(1)
                        .text(format!(
                            "{}{}",
                            c.model,
                            c.worktree.as_ref().map(|w| format!(" · {}", w.branch)).unwrap_or_default(),
                        ))
                        .font_size(10.5)
                        .color(th.faint()),
                );
            rail = rail.child(
                OxideTooltip::detailed(detail)
                    .placement(AttachedPosition::Right)
                    .offset(6.)
                    .child(
                        rect()
                            .width(Size::px(28.)).height(Size::px(28.))
                            .corner_radius(CornerRadius::new_all(8.))
                            .center()
                            .background(if sel { th.surface_hi() } else { th.surface() })
                            .on_press(move |_: Event<PressEventData>| {
                                st.open_conversation(id.clone());
                            })
                            .child(StatusDot::new(true)),
                    ),
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

        if is_collapsed {
            TooltipGroup::new().child(rail.into_element()).into_element()
        } else {
            col.into_element()
        }
    }
}
