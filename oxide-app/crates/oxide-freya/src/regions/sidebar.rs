//! Left region: project header + styled conversation list + collapse rail.
//!
//! The placeholder-page nav demo from 2a has been removed. The per-region-nav
//! independence invariant is covered by `nav::tests::region_nav_is_independent`.
use freya::prelude::*;
use freya_icons::lucide;
use oxide_ui::{
    Theme,
    components::{
        AttachedPosition, ConfirmDialog, ListItem, MenuRow, MenuSurface, OxideTooltip, Placement,
        Popover, RailButton, SidebarHeader, TextInput, TooltipGroup, menu_theme, open_context_menu,
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

        // Rename state: id of the conversation currently being edited (if any)
        // and the current draft title. Both are unconditional hooks.
        let mut editing  = use_state::<Option<String>>(|| None);
        let mut edit_val = use_state(String::new);

        // Icon picker: id of the conversation whose picker popover is open.
        let mut icon_pick = use_state::<Option<String>>(|| None);

        // Confirm-delete: Some((id, display_title)) when awaiting user confirmation.
        let mut confirm_delete = use_state::<Option<(oxide_client::ConversationId, String)>>(|| None);

        // Pending-delete: the actual `delete_conversation` spawn must be owned by THIS
        // (sidebar) scope, which stays mounted — not the ConfirmDialog's scope, which the
        // confirm handler unmounts in the same tick (that would cancel the un-polled task).
        // So the dialog only sets this signal; the side-effect below performs the delete.
        let mut pending_delete = use_state::<Option<oxide_client::ConversationId>>(|| None);
        {
            let st = state.clone();
            let mut pd = pending_delete;
            use_side_effect(move || {
                // Read into a local so the `.read()` guard is dropped before we `.set(None)`
                // below (an `if let pd.read()` would hold the borrow across the body → panic).
                let next = pd.read().clone();
                if let Some(id) = next {
                    st.delete_conversation(id);
                    pd.set(None);
                }
            });
        }

        let convs = state.conversations.read().clone();
        let active = state.active.read().clone();
        let meta = state.conversation_meta.read().clone();

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
            let st  = state.clone();
            let id  = c.id.clone();
            let sel = active.as_ref() == Some(&c.id);
            let m = meta.get(c.id.0.as_str());
            let title = crate::conversation_meta::effective_title(
                m.and_then(|x| x.title.as_deref()),
                &c.title,
            );
            let icon = crate::conversation_meta::effective_icon(
                m.and_then(|x| x.icon.as_deref()),
            );

            let row: Element = if editing.read().as_deref() == Some(c.id.0.as_str()) {
                // ── Inline rename editor ──────────────────────────────────────
                let id_commit = c.id.0.clone();
                let st_commit = state.clone();
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(2., 4., 2., 4.))
                    .child(
                        TextInput::new(edit_val.into_writable(), th)
                            .on_submit(move |text: String| {
                                st_commit.rename_conversation(&id_commit, text);
                                editing.set(None);
                            }),
                    )
                    .into_element()
            } else {
                // ── Normal row with right-click to open rename/icon menu ──────
                let id_menu    = c.id.0.clone();
                let id_pick    = c.id.0.clone();
                let title_seed = title.to_owned();
                // Pre-compute unsent status so it can be captured by value in the closure.
                let meta_title_str = m.and_then(|x| x.title.clone());
                let unsent_del = crate::conversation_meta::is_unsent(
                    &c.title,
                    meta_title_str.as_deref(),
                );
                let id_del    = c.id.clone();
                let title_del = title.to_owned();
                // Pre-clone state for use inside the move closure.
                let state_del = state.clone();
                let row_inner = rect()
                    .width(Size::fill())
                    .on_secondary_down(move |e: Event<PressEventData>| {
                        let (container_theme, item_theme) = menu_theme(th);
                        let item_theme2 = item_theme.clone();
                        let item_theme3 = item_theme.clone();
                        let id_press    = id_menu.clone();
                        let seed        = title_seed.clone();
                        let id_icon     = id_press.clone();
                        let st_del      = state_del.clone();
                        let id_del_c    = id_del.clone();
                        let title_del_c = title_del.clone();
                        let unsent      = unsent_del;
                        let menu = Menu::new()
                            .theme(container_theme)
                            .child(
                                MenuButton::new()
                                    .theme(item_theme)
                                    .on_press(move |_: Event<PressEventData>| {
                                        edit_val.set(seed.clone());
                                        editing.set(Some(id_press.clone()));
                                    })
                                    .child("Rename"),
                            )
                            .child(
                                MenuButton::new()
                                    .theme(item_theme2)
                                    .on_press(move |_: Event<PressEventData>| {
                                        icon_pick.set(Some(id_icon.clone()));
                                    })
                                    .child("Set icon\u{2026}"),
                            )
                            .child(
                                MenuButton::new()
                                    .theme(item_theme3)
                                    .on_press(move |_: Event<PressEventData>| {
                                        if unsent {
                                            st_del.delete_conversation(id_del_c.clone());
                                        } else {
                                            confirm_delete.set(Some((id_del_c.clone(), title_del_c.clone())));
                                        }
                                    })
                                    .child("Delete"),
                            );
                        open_context_menu(&e, menu);
                    })
                    .child(
                        ListItem::new(title)
                            .icon(Some(crate::conversation_meta::icon_svg(icon)))
                            .selected(sel)
                            .state("idle".into())
                            .worktree(c.worktree.as_ref().map(|w| w.branch.clone()))
                            .theme(th)
                            .on_press(move |_| st.open_conversation(id.clone())),
                    )
                    .into_element();

                // ── Icon picker popover ───────────────────────────────────────
                if icon_pick.read().as_deref() == Some(c.id.0.as_str()) {
                    let mut icon_rows = rect().direction(Direction::Vertical).padding(Gaps::new_all(4.));
                    for chunk in crate::conversation_meta::CURATED_ICONS.chunks(4) {
                        let mut icon_row = rect()
                            .direction(Direction::Horizontal)
                            .spacing(4.);
                        for &icon_name in chunk {
                            let st_ic = state.clone();
                            let id_ic = id_pick.clone();
                            let name  = icon_name.to_string();
                            icon_row = icon_row.child(
                                rect()
                                    .width(Size::px(30.))
                                    .height(Size::px(30.))
                                    .corner_radius(CornerRadius::new_all(8.))
                                    .center()
                                    .background(th.surface())
                                    .on_press(move |_: Event<PressEventData>| {
                                        st_ic.set_conversation_icon(&id_ic, name.clone());
                                        icon_pick.set(None);
                                    })
                                    .child(
                                        svg(crate::conversation_meta::icon_svg(icon_name))
                                            .width(Size::px(18.))
                                            .height(Size::px(18.))
                                            .color(th.text()),
                                    ),
                            );
                        }
                        icon_rows = icon_rows.child(icon_row);
                    }
                    let surface = MenuSurface::new(th)
                        .on_close({
                            let mut p = icon_pick;
                            move |()| p.set(None)
                        })
                        .child(icon_rows);
                    Popover::new(row_inner)
                        .open(true)
                        .placement(Placement::Below)
                        .content(surface)
                        .into_element()
                } else {
                    row_inner
                }
            };

            list = list.child(row);
        }

        col = col.child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.0))
                .padding(Gaps::new(0., 8., 0., 8.))
                .child(ScrollView::new().show_scrollbar(false).child(list)),
        );

        // ── Confirm-delete dialog (full-window overlay, only when Some) ────────
        col = col.maybe_child(confirm_delete.read().clone().map(|(del_id, del_title)| {
            ConfirmDialog::new(th)
                .title(format!("Delete \"{del_title}\"?"))
                .body("This can't be undone.".to_string())
                .confirm_label("Delete")
                .danger(true)
                .on_confirm((move |()| {
                    // Hand the delete to the sidebar-scoped side-effect (survives this
                    // dialog unmounting), then close the dialog.
                    pending_delete.set(Some(del_id.clone()));
                    confirm_delete.set(None);
                }).into())
                .on_cancel((move |()| {
                    confirm_delete.set(None);
                }).into())
        }));

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
            let m = meta.get(c.id.0.as_str());
            let title = crate::conversation_meta::effective_title(
                m.and_then(|x| x.title.as_deref()),
                &c.title,
            );
            let icon = crate::conversation_meta::effective_icon(
                m.and_then(|x| x.icon.as_deref()),
            );
            let detail = rect()
                .direction(Direction::Vertical)
                .spacing(3.)
                .child(label().max_lines(1).text(title).font_size(12.5).color(th.text()))
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
                            .child(
                                svg(crate::conversation_meta::icon_svg(icon))
                                    .width(Size::px(16.))
                                    .height(Size::px(16.))
                                    .color(if sel { th.text() } else { th.faint() }),
                            ),
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
