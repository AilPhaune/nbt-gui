use std::{collections::HashMap, hash::Hash, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{Context, anyhow};
use egui::{
    Align, FontFamily, Id, Key, Label, Layout, RichText, Sense, Spinner, Stroke, TextEdit, Ui,
    Widget,
};
use egui_dock::{TabViewer, tab_viewer::OnCloseResponse};
use egui_ltreeview::{NodeBuilder, TreeView, TreeViewBuilder};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use iconflow::{IconError, Pack, Size, Style, try_icon};
use simdnbt::{
    Mutf8String,
    owned::{BaseNbt, Nbt, NbtCompound, NbtList, NbtTag},
};

use crate::{
    i18n::Translations,
    ui::document::{DocumentData, NbtCompression, NbtDocumentTab},
};

pub trait NbtValue: Sized + 'static {
    fn nbt_from_str(s: String) -> Option<Self>;
    fn nbt_to_str(&self) -> String;
}

impl NbtValue for Mutf8String {
    fn nbt_from_str(s: String) -> Option<Self> {
        Some(Mutf8String::from_string(s))
    }

    fn nbt_to_str(&self) -> String {
        self.to_string()
    }
}

impl NbtValue for () {
    fn nbt_from_str(_: String) -> Option<Self> {
        Some(())
    }

    fn nbt_to_str(&self) -> String {
        String::new()
    }
}

macro_rules! impl_fromstr2 {
    ($($t:ty),*) => {
        $(impl NbtValue for $t {
            fn nbt_from_str(s: String) -> Option<Self> {
                s.parse().ok()
            }

            fn nbt_to_str(&self) -> String {
                self.to_string()
            }
        })*
    };
}

impl_fromstr2!(i8, u8, i16, i32, i64, f32, f64, bool);

pub trait NbtValueTo<T> {
    fn to(self) -> T;
}

impl<S, D> NbtValueTo<D> for &S
where
    D: TryFrom<S> + Default,
    S: Copy,
{
    fn to(self) -> D {
        D::try_from(*self).unwrap_or_default()
    }
}

impl<S, D> NbtValueTo<D> for &mut S
where
    D: TryFrom<S> + Default,
    S: Copy,
{
    fn to(self) -> D {
        D::try_from(*self).unwrap_or_default()
    }
}

pub enum TabEvent {
    OpenNewFileTab {
        path: PathBuf,
    },
    OpenNewTab,
    DoneOpeningNewTabFile {
        tab_id: usize,
        data: anyhow::Result<DocumentData>,
    },
    SaveFileTabResult {
        tab_id: usize,
        path: Option<PathBuf>,
        data: DocumentData,
        result: anyhow::Result<()>,
    },
}

#[derive(Debug, Clone)]
pub enum NbtClipboard {
    CompoundEntry(Mutf8String, NbtTag),
    ListEntry(NbtTag),
}

enum BaseContextMenuAction {
    Delete,
    Copy,
    Cut,
    Paste,
}

pub struct NbtTabViewer {
    pub per_short_title: HashMap<String, u32>,
    pub events_rx: UnboundedReceiver<TabEvent>,
    pub events_tx: UnboundedSender<TabEvent>,
    pub last_tab_id: usize,
    pub translations: Arc<Translations>,
    pub icon_base_nbt: (char, FontFamily),
    pub icon_nothing: (char, FontFamily),
    pub icon_compound_nbt: (char, FontFamily),
    pub icon_numeric: (char, FontFamily),
    pub icon_string: (char, FontFamily),
    pub icon_list: (char, FontFamily),
    pub icon_array: (char, FontFamily),

    pub clipboard: Option<NbtClipboard>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct NbtNodeId {
    parent: Arc<Vec<usize>>,
    idx: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NbtNodeChilds {
    parent: Arc<Vec<usize>>,
    pos: usize,
}

impl NbtNodeId {
    fn childs(self) -> NbtNodeChilds {
        let new_parent = Arc::new([self.parent.as_slice(), &[self.idx]].concat());
        NbtNodeChilds {
            parent: new_parent,
            pos: 0,
        }
    }
}

impl Iterator for NbtNodeChilds {
    type Item = NbtNodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.nth(0)
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        Some(NbtNodeId {
            parent: self.parent.clone(),
            idx: {
                let i = self.pos.checked_add(n)?;
                self.pos = i.checked_add(1)?;
                i
            },
        })
    }
}

fn get_icon(
    pack: Pack,
    name: &str,
    style: Style,
    size: Size,
) -> anyhow::Result<(char, FontFamily)> {
    let icon = try_icon(pack, name, style, size).map_err(|e| match e {
        IconError::IconNotFound { pack, name } => {
            anyhow!("Icon not found: {} in {:?}", name, pack)
        }
        IconError::PackDisabled { pack } => {
            anyhow!(
                "Icon {} can't be loaded because {:?} is disabled",
                name,
                pack
            )
        }
        IconError::VariantUnavailable {
            pack,
            name,
            requested,
            ..
        } => {
            anyhow!(
                "Icon {} from {:?} isn't available in the requested {:?} and {:?}",
                name,
                pack,
                requested.0,
                requested.1
            )
        }
    })?;
    let c = char::from_u32(icon.codepoint).with_context(|| {
        format!(
            "Invalid codepoint {} for icon {} from {:?} with {:?} and {:?}",
            icon.codepoint, name, pack, style, size
        )
    })?;
    Ok((c, FontFamily::Name(icon.family.into())))
}

fn nbt_list_type_hint<'t>(list: &NbtList, translations: &'t Translations) -> &'t str {
    match list {
        NbtList::Byte(_) => &translations.c().type_hint_list_i8,
        NbtList::ByteArray(_) => &translations.c().type_hint_list_byte_arrays,
        NbtList::Compound(_) => &translations.c().type_hint_list_compounds,
        NbtList::Double(_) => &translations.c().type_hint_list_f64,
        NbtList::Empty => &translations.c().type_hint_empty_list,
        NbtList::Float(_) => &translations.c().type_hint_list_f32,
        NbtList::Int(_) => &translations.c().type_hint_list_i32,
        NbtList::IntArray(_) => &translations.c().type_hint_list_int_arrays,
        NbtList::List(_) => &translations.c().type_hint_list_lists,
        NbtList::Long(_) => &translations.c().type_hint_list_i64,
        NbtList::LongArray(_) => &translations.c().type_hint_list_long_arrays,
        NbtList::Short(_) => &translations.c().type_hint_list_i16,
        NbtList::String(_) => &translations.c().type_hint_list_strs,
    }
}

fn nbt_list_len(list: &NbtList) -> usize {
    match list {
        NbtList::Empty => 0,

        NbtList::Byte(vals) => vals.len(),
        NbtList::ByteArray(vals) => vals.len(),
        NbtList::Compound(vals) => vals.len(),
        NbtList::Double(vals) => vals.len(),
        NbtList::Float(vals) => vals.len(),
        NbtList::Int(vals) => vals.len(),
        NbtList::IntArray(vals) => vals.len(),
        NbtList::List(vals) => vals.len(),
        NbtList::Long(vals) => vals.len(),
        NbtList::LongArray(vals) => vals.len(),
        NbtList::Short(vals) => vals.len(),
        NbtList::String(vals) => vals.len(),
    }
}

macro_rules! conv_warn {
    ($t: expr, $ui: ident) => {
        $ui.allocate_ui_with_layout(egui::vec2(300.0, 0.0), Layout::top_down(Align::Min), |ui| {
            Label::new(&*$t.c().dt_conv_warn)
                .sense(Sense::empty())
                .selectable(false)
                .wrap()
                .ui(ui);
        });
    };
}

struct EntryContext<'val, 'key, 'extra, 'icon, 'th, T, ContextMenuFn: FnMut(&mut Ui)> {
    val: Option<&'val T>,
    key: Option<&'key str>,
    idx: Option<usize>,
    extra: Option<&'extra str>,
    icon: &'icon (char, FontFamily),
    type_hint: &'th str,
    context_menu: ContextMenuFn,
}

impl NbtTabViewer {
    pub fn new(translations: Arc<Translations>) -> anyhow::Result<Self> {
        let (events_tx, events_rx) = unbounded();

        Ok(Self {
            per_short_title: HashMap::new(),
            last_tab_id: 0,
            icon_base_nbt: get_icon(Pack::Lucide, "folder", Style::Regular, Size::Regular)?,
            icon_nothing: get_icon(
                Pack::Lucide,
                "file-plus-corner",
                Style::Regular,
                Size::Regular,
            )?,
            icon_compound_nbt: get_icon(Pack::Lucide, "braces", Style::Regular, Size::Regular)?,
            icon_numeric: get_icon(Pack::Lucide, "calculator", Style::Regular, Size::Regular)?,
            icon_string: get_icon(
                Pack::Lucide,
                "case-sensitive",
                Style::Regular,
                Size::Regular,
            )?,
            icon_list: get_icon(Pack::Lucide, "list", Style::Regular, Size::Regular)?,
            icon_array: get_icon(Pack::Lucide, "brackets", Style::Regular, Size::Regular)?,
            translations,
            events_rx,
            events_tx,

            clipboard: None,
        })
    }

    pub fn next_tab_id(&mut self) -> Option<usize> {
        let (next, overflow) = self.last_tab_id.overflowing_add(1);
        if overflow {
            None
        } else {
            self.last_tab_id = next;
            Some(next)
        }
    }

    pub fn insert(&mut self, tab: &mut NbtDocumentTab) -> bool {
        self.next_tab_id()
            .map(|id| {
                if let Some(count) = self.per_short_title.get_mut(&tab.title_short) {
                    let (ncount, overflow) = count.overflowing_add(1);
                    if overflow {
                        false
                    } else {
                        *count = ncount;
                        tab.update_id(id);
                        true
                    }
                } else {
                    self.per_short_title.insert(tab.title_short.clone(), 1);
                    tab.update_id(id);
                    true
                }
            })
            .unwrap_or(false)
    }

    fn add_base_context_menu(&self, ui: &mut Ui, can_paste: bool) -> Option<BaseContextMenuAction> {
        if ui
            .button(&*self.translations.c().button_delete_text)
            .clicked()
        {
            return Some(BaseContextMenuAction::Delete);
        }

        if ui.button(&*self.translations.c().button_cut_text).clicked() {
            return Some(BaseContextMenuAction::Cut);
        }

        if ui
            .button(&*self.translations.c().button_copy_text)
            .clicked()
        {
            return Some(BaseContextMenuAction::Copy);
        }

        if ui
            .add_enabled_ui(can_paste, |ui| {
                ui.button(&*self.translations.c().button_paste_text)
            })
            .inner
            .clicked()
        {
            return Some(BaseContextMenuAction::Paste);
        }

        None
    }

    fn editable_str_label(
        ui: &mut egui::Ui,
        id: egui::Id,
        current: &str,
        text_empty: &str,
    ) -> Option<String> {
        let editing_id = Id::new("nbt_tree_currently_editing");
        let buffer_id = id.with("buf");

        let editing = ui.memory(|m| m.data.get_temp::<Id>(editing_id));

        let just_done_editing_id = id.with("just_done");
        let just_started_editing_id = Id::new("id_elem_just_started_editing");

        if editing != Some(id) {
            let is_empty = current.is_empty();
            let text = if is_empty {
                RichText::new(text_empty)
                    .italics()
                    .color(ui.visuals().weak_text_color())
            } else {
                RichText::new(current.to_string()).color(ui.visuals().text_color())
            };

            let response = ui
                .scope(|ui| {
                    ui.visuals_mut().widgets.active.fg_stroke = Stroke::NONE;

                    Label::new(text).sense(Sense::click()).ui(ui)
                })
                .inner;

            let should_request_focus = ui
                .memory(|m| m.data.get_temp::<bool>(just_done_editing_id))
                .unwrap_or(false);

            if should_request_focus {
                response.request_focus();
                ui.memory_mut(|m| m.data.insert_temp(just_done_editing_id, false));
            }

            let underline_color = if response.hovered() {
                ui.visuals().widgets.hovered.bg_stroke.color
            } else if response.has_focus() {
                ui.visuals().widgets.active.fg_stroke.color
            } else {
                ui.visuals().weak_text_color().gamma_multiply(0.5)
            };
            ui.painter().hline(
                response.rect.x_range(),
                response.rect.bottom(),
                Stroke::new(1.0, underline_color),
            );

            if response.double_clicked() && editing.is_none() {
                ui.memory_mut(|m| {
                    m.data.insert_temp(buffer_id, current.to_string());
                    m.data.insert_temp(editing_id, id);
                    m.data.remove::<Id>(just_started_editing_id);
                });
                ui.ctx().request_repaint();
            }
            return None;
        }

        let mut buffer = ui
            .memory_mut(|m| m.data.get_temp::<String>(buffer_id))
            .unwrap_or_else(|| current.to_string());

        let response = ui.add(
            TextEdit::singleline(&mut buffer)
                .id(id.with("text_edit"))
                .hint_text(text_empty),
        );

        let already_requested =
            ui.memory(|m| m.data.get_temp::<Id>(just_started_editing_id)) == Some(id);

        if !already_requested {
            response.request_focus();
            ui.memory_mut(|m| m.data.insert_temp(just_started_editing_id, id));
        }

        ui.memory_mut(|m| m.data.insert_temp(buffer_id, buffer.clone()));

        let committed = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
        let blurred = response.lost_focus() && !committed && !cancelled;

        if committed || cancelled || blurred {
            ui.memory_mut(|m| {
                m.data.remove::<String>(buffer_id);
                m.data.remove::<Id>(editing_id);
                m.data.insert_temp(just_done_editing_id, true);
            });
        }

        if committed || blurred {
            Some(buffer)
        } else {
            None
        }
    }

    fn build_base_nbt_label(&self, ui: &mut Ui, id: Id, nbt: &mut BaseNbt) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            Label::new(
                RichText::new(self.icon_base_nbt.0)
                    .family(self.icon_base_nbt.1.clone())
                    .color(ui.visuals().text_color()),
            )
            .sense(Sense::empty())
            .selectable(false)
            .ui(ui);

            if let Some(new_name) = Self::editable_str_label(
                ui,
                id,
                nbt.name().to_str().as_ref(),
                &self.translations.c().unnamed_root_nbt_text_hint,
            ) {
                let old = std::mem::take(nbt);
                let tag = old.as_compound();
                *nbt = BaseNbt::new(Mutf8String::from_string(new_name), tag);
            }
        });
    }

    fn show_nbt_tree(
        &mut self,
        nbt: &mut Nbt,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) {
        match nbt {
            Nbt::None => {
                builder.node(
                    NodeBuilder::leaf(NbtNodeId::default())
                        .label_ui(|ui| {
                            ui.horizontal(|ui| {
                                Label::new(
                                    RichText::new(self.icon_nothing.0)
                                        .family(self.icon_nothing.1.clone())
                                        .color(ui.visuals().text_color()),
                                )
                                .sense(Sense::empty())
                                .selectable(false)
                                .ui(ui);

                                Label::new(&*self.translations.c().root_nbt_empty_text)
                                    .sense(Sense::empty())
                                    .selectable(false)
                                    .ui(ui);
                            });
                        })
                        .context_menu(|ui| {
                            if ui
                                .button(&*self.translations.c().button_create_empty_root)
                                .clicked()
                            {
                                *nbt = Nbt::Some(BaseNbt::new("", NbtCompound::new()));
                            }

                            if ui
                                .add_enabled_ui(self.clipboard.is_some(), |ui| {
                                    ui.button(&*self.translations.c().button_paste_text)
                                })
                                .inner
                                .clicked()
                            {
                                match &self.clipboard {
                                    Some(NbtClipboard::CompoundEntry(name, tag)) => match tag {
                                        NbtTag::Compound(c) => {
                                            *nbt = Nbt::Some(BaseNbt::new(name.clone(), c.clone()));
                                        }
                                        NbtTag::Byte(_)
                                        | NbtTag::ByteArray(_)
                                        | NbtTag::Double(_)
                                        | NbtTag::Float(_)
                                        | NbtTag::Int(_)
                                        | NbtTag::IntArray(_)
                                        | NbtTag::List(_)
                                        | NbtTag::Long(_)
                                        | NbtTag::LongArray(_)
                                        | NbtTag::Short(_)
                                        | NbtTag::String(_) => {
                                            *nbt = Nbt::Some(BaseNbt::new(
                                                "",
                                                NbtCompound::from_values(vec![(
                                                    name.clone(),
                                                    tag.clone(),
                                                )]),
                                            ));
                                        }
                                    },
                                    Some(NbtClipboard::ListEntry(value)) => {
                                        *nbt = Nbt::Some(BaseNbt::new(
                                            "",
                                            NbtCompound::from_values(vec![(
                                                "".into(),
                                                value.clone(),
                                            )]),
                                        ));
                                    }
                                    None => {}
                                }
                            }
                        }),
                );
            }
            Nbt::Some(bnbt) => {
                let mut base_action = None;

                let dir_open = builder.node(
                    NodeBuilder::dir(NbtNodeId::default())
                        .label_ui(|ui| {
                            self.build_base_nbt_label(ui, egui_id, bnbt);
                        })
                        .context_menu(|ui| {
                            base_action = self.add_base_context_menu(ui, self.clipboard.is_some());
                        })
                        .default_open(false),
                );

                if dir_open {
                    self.show_compound_tree(
                        &mut *bnbt,
                        NbtNodeId::default().childs(),
                        egui_id.with(0),
                        builder,
                    );
                }

                builder.close_dir();

                match base_action {
                    Some(BaseContextMenuAction::Delete) => {
                        *nbt = Nbt::None;
                    }
                    Some(BaseContextMenuAction::Cut) => {
                        self.clipboard = Some(NbtClipboard::CompoundEntry(
                            Mutf8String::from(bnbt.name()),
                            NbtTag::Compound(std::mem::take(bnbt).as_compound()),
                        ));
                        *nbt = Nbt::None;
                    }
                    Some(BaseContextMenuAction::Copy) => {
                        self.clipboard = Some(NbtClipboard::CompoundEntry(
                            Mutf8String::from(bnbt.name()),
                            NbtTag::Compound(bnbt.clone().as_compound()),
                        ));
                    }
                    Some(BaseContextMenuAction::Paste) => match self.clipboard.clone() {
                        None => {}
                        Some(NbtClipboard::CompoundEntry(name, tag)) => {
                            bnbt.insert(name, tag);
                        }
                        Some(NbtClipboard::ListEntry(tag)) => {
                            bnbt.insert("", tag);
                        }
                    },
                    None => {}
                }
            }
        }
    }

    fn show_entry<T: NbtValue>(
        &self,
        id: NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
        EntryContext {
            val,
            key,
            idx,
            extra,
            icon,
            type_hint,
            context_menu,
        }: EntryContext<'_, '_, '_, '_, '_, T, impl FnMut(&mut Ui)>,
    ) -> (bool, Option<String>, Option<T>) {
        let mut ret = None;
        let mut ret_new_val = None;

        let node_base = match val {
            Some(_) => NodeBuilder::leaf(id.clone()),
            None => NodeBuilder::dir(id.clone()).default_open(false),
        };

        let open = builder.node(
            node_base
                .label_ui(|ui| {
                    ui.horizontal(|ui| {
                        Label::new(
                            RichText::new(icon.0)
                                .family(icon.1.clone())
                                .color(ui.visuals().text_color()),
                        )
                        .sense(Sense::empty())
                        .selectable(false)
                        .show_tooltip_when_elided(false)
                        .ui(ui);

                        if let Some(key) = key {
                            ret = Self::editable_str_label(
                                ui,
                                egui_id.with("editable-key"),
                                key,
                                &self.translations.c().editable_key_empty_text,
                            );
                        }

                        if let Some(idx) = idx {
                            Label::new(
                                RichText::new(idx.to_string()).color(ui.visuals().text_color()),
                            )
                            .sense(Sense::empty())
                            .show_tooltip_when_elided(false)
                            .ui(ui);
                        }

                        if let Some(val) = val {
                            Label::new(RichText::new(":").color(ui.visuals().text_color()))
                                .sense(Sense::empty())
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .ui(ui);

                            if let Some(new_val) = Self::editable_str_label(
                                ui,
                                egui_id.with("editable-value"),
                                &val.nbt_to_str(),
                                &self.translations.c().editable_value_empty_text,
                            ) && let Some(parsed) = T::nbt_from_str(new_val)
                            {
                                ret_new_val = Some(parsed);
                            }
                        }

                        if let Some(extra) = extra {
                            Label::new(RichText::new(extra).color(ui.visuals().text_color()))
                                .ui(ui);
                        }
                    })
                    .response
                    .interact(Sense::hover())
                    .on_hover_text(type_hint);
                })
                .context_menu(context_menu),
        );

        (open, ret, ret_new_val)
    }

    fn show_list<T>(
        &mut self,
        child_ids: NbtNodeChilds,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
        values: &mut [T],
        mut renderer: impl FnMut(
            &mut NbtTabViewer,
            NbtNodeId,
            Id,
            &mut TreeViewBuilder<NbtNodeId>,
            usize,
            &mut T,
        ),
    ) {
        for ((idx, value), child_id) in values.iter_mut().enumerate().zip(child_ids) {
            let egui_id = egui_id.with(idx);
            renderer(self, child_id, egui_id, builder, idx, value);
        }
    }

    fn show_nbt_list(
        &mut self,
        nbt: &mut NbtList,
        child_ids: NbtNodeChilds,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) -> Option<NbtTag> {
        macro_rules! simple_view_list {
            ($values: ident, $icon: ident, $th: ident) => {
                self.show_list(
                    child_ids,
                    egui_id,
                    builder,
                    $values,
                    |tab_viewer, id, egui_id, builder, idx, value| {
                        let (_, _, new_value) = tab_viewer.show_entry(
                            id,
                            egui_id,
                            builder,
                            EntryContext {
                                val: Some(value),
                                key: None,
                                idx: Some(idx),
                                extra: None,
                                icon: &tab_viewer.$icon,
                                type_hint: &*tab_viewer.translations.c().$th,
                                context_menu: |ui| {
                                    // TODO: List element context menu
                                    ui.label("TODO: List element context menu");
                                },
                            },
                        );

                        if let Some(new_value) = new_value {
                            *value = new_value;
                        }
                    },
                )
            };
        }

        macro_rules! simple_view_list_list {
            ($values: ident, $th: ident, $th_elem: ident) => {{
                self.show_list(
                    child_ids,
                    egui_id,
                    builder,
                    $values,
                    |tab_viewer, id, egui_id, builder, idx, value| {
                        let (open, _, _) = tab_viewer.show_entry::<()>(
                            id.clone(),
                            egui_id,
                            builder,
                            EntryContext {
                                val: None,
                                key: None,
                                idx: Some(idx),
                                extra: Some(&tab_viewer.translations.f(
                                    "list-element-count",
                                    &HashMap::from([("count".into(), value.len().into())]),
                                )),
                                icon: &tab_viewer.icon_array,
                                type_hint: &*tab_viewer.translations.c().$th,
                                context_menu: |ui| {
                                    // TODO: List of arrays subarray context menu
                                    ui.label("TODO: List of arrays subarray context menu");
                                },
                            },
                        );

                        if open {
                            tab_viewer.show_list(
                                id.childs(),
                                egui_id,
                                builder,
                                value,
                                |tab_viewer, id, egui_id, builder, idx, value| {
                                    let (_, _, new_value) = tab_viewer.show_entry(
                                        id,
                                        egui_id,
                                        builder,
                                        EntryContext {
                                            val: Some(value),
                                            key: None,
                                            idx: Some(idx),
                                            extra: None,
                                            icon: &tab_viewer.icon_numeric,
                                            type_hint: &*tab_viewer.translations.c().$th_elem,
                                            context_menu: |ui| {
                                                // TODO: List of arrays subelement context menu
                                                ui.label(
                                                    "TODO: List of arrays subelement context menu",
                                                );
                                            },
                                        },
                                    );

                                    if let Some(new_value) = new_value {
                                        *value = new_value;
                                    }
                                },
                            )
                        }

                        builder.close_dir();
                    },
                );
            }};
        }

        let mut edit_value = None;

        match nbt {
            NbtList::Empty => unreachable!("This is a bug!"),
            NbtList::Byte(bs) => simple_view_list!(bs, icon_numeric, type_hint_i8),
            NbtList::Short(shs) => simple_view_list!(shs, icon_numeric, type_hint_i16),
            NbtList::Int(is) => simple_view_list!(is, icon_numeric, type_hint_i32),
            NbtList::Long(ls) => simple_view_list!(ls, icon_numeric, type_hint_i64),
            NbtList::Float(fs) => simple_view_list!(fs, icon_numeric, type_hint_f32),
            NbtList::Double(ds) => simple_view_list!(ds, icon_numeric, type_hint_f64),
            NbtList::String(strs) => simple_view_list!(strs, icon_string, type_hint_str),
            NbtList::ByteArray(bas) => {
                simple_view_list_list!(bas, type_hint_byte_array, type_hint_u8)
            }
            NbtList::IntArray(ias) => {
                simple_view_list_list!(ias, type_hint_int_array, type_hint_i32)
            }
            NbtList::LongArray(las) => {
                simple_view_list_list!(las, type_hint_long_array, type_hint_i64)
            }
            NbtList::List(ls) => self.show_list(
                child_ids,
                egui_id,
                builder,
                ls,
                |tab_viewer, id, egui_id, builder, idx, value| {
                    let list_len = nbt_list_len(value);

                    if matches!(value, NbtList::Empty) {
                        builder.node(NodeBuilder::leaf(id.clone()).label_ui(|ui| {
                            ui.horizontal(|ui| {
                                Label::new(
                                    RichText::new(tab_viewer.icon_list.0)
                                        .family(tab_viewer.icon_list.1.clone())
                                        .color(ui.visuals().text_color()),
                                )
                                .sense(Sense::empty())
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .ui(ui);

                                ui.label(
                                    RichText::new(idx.to_string()).color(ui.visuals().text_color()),
                                );

                                Label::new(":")
                                    .sense(Sense::empty())
                                    .selectable(false)
                                    .show_tooltip_when_elided(false)
                                    .ui(ui);

                                Label::new(
                                    RichText::new(&*tab_viewer.translations.c().empty_list_text)
                                        .color(ui.visuals().text_color()),
                                )
                                .ui(ui);
                            })
                            .response
                            .interact(Sense::hover())
                            .on_hover_text(nbt_list_type_hint(value, &tab_viewer.translations));
                        }));

                        return;
                    }

                    let (open, _, _) = tab_viewer.show_entry::<()>(
                        id.clone(),
                        egui_id,
                        builder,
                        EntryContext {
                            val: None,
                            key: None,
                            idx: Some(idx),
                            extra: Some(&tab_viewer.translations.f(
                                "list-element-count",
                                &HashMap::from([("count".into(), list_len.into())]),
                            )),
                            icon: &tab_viewer.icon_list,
                            type_hint: nbt_list_type_hint(value, &tab_viewer.translations),
                            context_menu: |ui| {
                                if let Some(to_tag) = tab_viewer
                                    .show_nbt_list_entry_context_menu_type_conversion(
                                        ui, value, false,
                                    )
                                {
                                    edit_value = Some(to_tag);
                                }
                            },
                        },
                    );

                    if open {
                        let _ = tab_viewer.show_nbt_list(value, id.childs(), egui_id, builder);
                    }

                    builder.close_dir();
                },
            ),
            NbtList::Compound(cs) => self.show_list(
                child_ids,
                egui_id,
                builder,
                cs,
                |tab_viewer, id, egui_id, builder, idx, value| {
                    let (open, _, _) = tab_viewer.show_entry::<()>(
                        id.clone(),
                        egui_id,
                        builder,
                        EntryContext {
                            val: None,
                            key: None,
                            idx: Some(idx),
                            extra: Some(&tab_viewer.translations.f(
                                "compound-keys-count",
                                &HashMap::from([("count".into(), value.iter().count().into())]),
                            )),
                            icon: &tab_viewer.icon_compound_nbt,
                            type_hint: &tab_viewer.translations.c().type_hint_compound,
                            context_menu: |ui| {
                                // TODO: Compound in list context menu
                                ui.label("TODDO: Compound in list context menu");
                            },
                        },
                    );

                    if open {
                        tab_viewer.show_compound_tree(value, id.childs(), egui_id, builder);
                    }

                    builder.close_dir();
                },
            ),
        }

        edit_value
    }

    fn show_nbt_list_entry_context_menu_type_conversion(
        &self,
        ui: &mut Ui,
        nbt: &mut NbtList,
        can_conv_to_tag: bool,
    ) -> Option<NbtTag> {
        let mut new_value = None;
        let mut convert_to_tag = None;

        macro_rules! convs {
            ($translation: ident, $convs: expr) => {{
                ui.menu_button(&*self.translations.c().$translation, $convs);
            }};
        }

        macro_rules! simple_conv_to {
            ($ui: ident, $vals: ident, $translation: ident, $variant: ident, $t: ident) => {{
                if $ui.button(&*self.translations.c().$translation).clicked() {
                    new_value = Some(NbtList::$variant($vals.iter().map(|e| *e as $t).collect()));
                }
            }};
        }

        macro_rules! array_to_array_conv {
            ($ui: ident, $vals: ident, $translation: ident, $variant: ident, $t: ident) => {{
                if $ui.button(&*self.translations.c().$translation).clicked() {
                    new_value = Some(NbtList::$variant(
                        $vals
                            .iter()
                            .map(|l| l.iter().map(|e| *e as $t).collect())
                            .collect(),
                    ));
                }
            }};
        }

        macro_rules! array_to_list_conv {
            ($ui: ident, $vals: ident, $translation: ident, $variant: ident, $t: ident) => {{
                if $ui.button(&*self.translations.c().$translation).clicked() {
                    new_value = Some(NbtList::List(
                        $vals
                            .iter()
                            .map(|l| NbtList::$variant(l.iter().map(|e| *e as $t).collect()))
                            .collect(),
                    ));
                }
            }};
        }

        macro_rules! list_list_to_array_conv {
            ($ui: ident, $vals: ident, $translation: ident, $variant: ident, $t: ident) => {{
                if $ui.button(&*self.translations.c().$translation).clicked() {
                    new_value = Some(NbtList::$variant(
                        $vals
                            .iter()
                            .map(|l| match l {
                                NbtList::Byte(bs) => bs.iter().map(|e| *e as $t).collect(),
                                NbtList::Short(shs) => shs.iter().map(|e| *e as $t).collect(),
                                NbtList::Int(is) => is.iter().map(|e| *e as $t).collect(),
                                NbtList::Long(ls) => ls.iter().map(|e| *e as $t).collect(),
                                NbtList::Float(fs) => fs.iter().map(|e| *e as $t).collect(),
                                NbtList::Double(ds) => ds.iter().map(|e| *e as $t).collect(),
                                NbtList::String(strs) => strs
                                    .iter()
                                    .map(|e| $t::from_str(e.to_str().as_ref()).unwrap_or_default())
                                    .collect(),
                                _ => vec![],
                            })
                            .collect(),
                    ));
                }
            }};
        }

        macro_rules! simple_conv_to_string {
            ($ui: ident, $vals: ident) => {{
                if $ui.button(&*self.translations.c().type_hint_str).clicked() {
                    new_value = Some(NbtList::String(
                        $vals
                            .iter()
                            .map(|e| Mutf8String::from(e.to_string()))
                            .collect(),
                    ));
                }
            }};
        }

        macro_rules! simple_conv_from_string {
            ($ui: ident, $vals: ident, $translation: ident, $variant: ident, $t: ident, $def: expr) => {{
                $ui.menu_button(&*self.translations.c().$translation, |ui| {
                    if ui
                        .button(&*self.translations.c().type_conv_abort_on_fail_text)
                        .clicked()
                    {
                        if let Ok(new_vals) = $vals
                            .iter()
                            .map(|e| $t::from_str(e.to_str().as_ref()))
                            .collect::<Result<Vec<$t>, _>>()
                        {
                            new_value = Some(NbtList::$variant(new_vals));
                        }
                    }

                    if ui
                        .button(&self.translations.f(
                            "type-conv-default-on-fail-text",
                            &HashMap::from([("def".into(), $def.into())]),
                        ))
                        .clicked()
                    {
                        new_value = Some(NbtList::$variant(
                            $vals
                                .iter()
                                .map(|e| $t::from_str(e.to_str().as_ref()).unwrap_or($def))
                                .collect(),
                        ));
                    }
                });
            }};
        }

        macro_rules! simple_empty_conv_to {
            ($ui: ident, $translation: ident, $variant: ident) => {
                if $ui.button(&*self.translations.c().$translation).clicked() {
                    new_value = Some(NbtList::$variant(vec![]));
                }
            };
        }

        macro_rules! conv_to_tag {
            ($ui: ident, $vals: expr, $variant: ident) => {
                if can_conv_to_tag
                    && ui
                        .button(&*self.translations.c().button_list_to_compound_conv)
                        .clicked()
                {
                    convert_to_tag = Some(NbtTag::Compound(NbtCompound::from_values(
                        std::mem::take($vals)
                            .into_iter()
                            .enumerate()
                            .map(|(idx, val)| {
                                (Mutf8String::from(idx.to_string()), NbtTag::$variant(val))
                            })
                            .collect(),
                    )));
                }
            };
        }

        match nbt {
            NbtList::Empty => {
                convs!(nbt_list_change_type, |ui| {
                    simple_empty_conv_to!(ui, type_hint_i8, Byte);
                    simple_empty_conv_to!(ui, type_hint_i16, Short);
                    simple_empty_conv_to!(ui, type_hint_i32, Int);
                    simple_empty_conv_to!(ui, type_hint_i64, Long);
                    simple_empty_conv_to!(ui, type_hint_f32, Float);
                    simple_empty_conv_to!(ui, type_hint_f64, Double);
                    simple_empty_conv_to!(ui, type_hint_str, String);
                    simple_empty_conv_to!(ui, type_hint_byte_array, ByteArray);
                    simple_empty_conv_to!(ui, type_hint_int_array, IntArray);
                    simple_empty_conv_to!(ui, type_hint_long_array, LongArray);
                    simple_empty_conv_to!(ui, type_hint_list, List);
                    simple_empty_conv_to!(ui, type_hint_compound, Compound);
                });
                let mut empty: [i8; 0] = [];
                conv_to_tag!(ui, &mut empty, Byte);
            }

            NbtList::Byte(bs) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, bs, type_hint_i16, Short, i16);
                    simple_conv_to!(ui, bs, type_hint_i32, Int, i32);
                    simple_conv_to!(ui, bs, type_hint_i64, Long, i64);
                    simple_conv_to!(ui, bs, type_hint_f32, Float, f32);
                    simple_conv_to!(ui, bs, type_hint_f64, Double, f64);
                    simple_conv_to_string!(ui, bs);
                });
                conv_to_tag!(ui, bs, Byte);
            }
            NbtList::Short(shs) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, shs, type_hint_i8, Byte, i8);
                    simple_conv_to!(ui, shs, type_hint_i32, Int, i32);
                    simple_conv_to!(ui, shs, type_hint_i64, Long, i64);
                    simple_conv_to!(ui, shs, type_hint_f32, Float, f32);
                    simple_conv_to!(ui, shs, type_hint_f64, Double, f64);
                    simple_conv_to_string!(ui, shs);
                });
                conv_to_tag!(ui, shs, Short);
            }
            NbtList::Int(is) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, is, type_hint_i8, Byte, i8);
                    simple_conv_to!(ui, is, type_hint_i16, Short, i16);
                    simple_conv_to!(ui, is, type_hint_i64, Long, i64);
                    simple_conv_to!(ui, is, type_hint_f32, Float, f32);
                    simple_conv_to!(ui, is, type_hint_f64, Double, f64);
                    simple_conv_to_string!(ui, is);
                });
                conv_to_tag!(ui, is, Int);
            }
            NbtList::Long(ls) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, ls, type_hint_i8, Byte, i8);
                    simple_conv_to!(ui, ls, type_hint_i16, Short, i16);
                    simple_conv_to!(ui, ls, type_hint_i32, Int, i32);
                    simple_conv_to!(ui, ls, type_hint_f32, Float, f32);
                    simple_conv_to!(ui, ls, type_hint_f64, Double, f64);
                    simple_conv_to_string!(ui, ls);
                });
                conv_to_tag!(ui, ls, Long);
            }
            NbtList::Float(fs) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, fs, type_hint_i8, Byte, i8);
                    simple_conv_to!(ui, fs, type_hint_i16, Short, i16);
                    simple_conv_to!(ui, fs, type_hint_i32, Int, i32);
                    simple_conv_to!(ui, fs, type_hint_i64, Long, i64);
                    simple_conv_to!(ui, fs, type_hint_f64, Double, f64);
                    simple_conv_to_string!(ui, fs);
                });
                conv_to_tag!(ui, fs, Float);
            }
            NbtList::Double(ds) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_to!(ui, ds, type_hint_i8, Byte, i8);
                    simple_conv_to!(ui, ds, type_hint_i16, Short, i16);
                    simple_conv_to!(ui, ds, type_hint_i32, Int, i32);
                    simple_conv_to!(ui, ds, type_hint_i64, Long, i64);
                    simple_conv_to!(ui, ds, type_hint_f32, Float, f32);
                    simple_conv_to_string!(ui, ds);
                });
                conv_to_tag!(ui, ds, Double);
            }
            NbtList::String(strs) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    simple_conv_from_string!(ui, strs, type_hint_i8, Byte, i8, 0);
                    simple_conv_from_string!(ui, strs, type_hint_i16, Short, i16, 0);
                    simple_conv_from_string!(ui, strs, type_hint_i32, Int, i32, 0);
                    simple_conv_from_string!(ui, strs, type_hint_i64, Long, i64, 0);
                    simple_conv_from_string!(ui, strs, type_hint_f32, Float, f32, f32::NAN);
                    simple_conv_from_string!(ui, strs, type_hint_f64, Double, f64, f64::NAN);
                });
                conv_to_tag!(ui, strs, String);
            }

            NbtList::ByteArray(bas) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    array_to_array_conv!(ui, bas, type_hint_int_array, IntArray, i32);
                    array_to_array_conv!(ui, bas, type_hint_long_array, LongArray, i64);
                    array_to_list_conv!(ui, bas, type_hint_list, Byte, i8);
                });
                conv_to_tag!(ui, bas, ByteArray);
            }
            NbtList::IntArray(ias) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    array_to_array_conv!(ui, ias, type_hint_byte_array, ByteArray, u8);
                    array_to_array_conv!(ui, ias, type_hint_long_array, LongArray, i64);
                    array_to_list_conv!(ui, ias, type_hint_list, Int, i32);
                });
                conv_to_tag!(ui, ias, IntArray);
            }
            NbtList::LongArray(las) => {
                convs!(nbt_list_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    array_to_array_conv!(ui, las, type_hint_byte_array, ByteArray, u8);
                    array_to_array_conv!(ui, las, type_hint_int_array, IntArray, i32);
                    array_to_list_conv!(ui, las, type_hint_list, Long, i64);
                });
                conv_to_tag!(ui, las, LongArray);
            }

            NbtList::List(ls) => {
                convs!(nbt_list_try_change_type, |ui| {
                    conv_warn!(self.translations, ui);
                    list_list_to_array_conv!(ui, ls, type_hint_byte_array, ByteArray, u8);
                    list_list_to_array_conv!(ui, ls, type_hint_int_array, IntArray, i32);
                    list_list_to_array_conv!(ui, ls, type_hint_long_array, LongArray, i64);
                });
                conv_to_tag!(ui, ls, List);
            }

            NbtList::Compound(cs) => {
                conv_to_tag!(ui, cs, Compound);
            }
        }

        if let Some(new_value) = new_value {
            *nbt = new_value;
        }

        convert_to_tag
    }

    fn show_compound_tree(
        &mut self,
        nbt: &mut NbtCompound,
        child_ids: NbtNodeChilds,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) {
        enum CopyPasteAction {
            Delete,
            Cut,
            Copy,
            ValueInPlace,
            TagAndValueInPlace,
            InsertAbove,
            InsertBelow,
        }

        let mut edit = None;
        let mut copy_paste = None;

        for ((idx, (key, tag)), child_id) in nbt.iter_mut().enumerate().zip(child_ids) {
            let egui_id = egui_id.with(idx);

            let mut update_type = None;

            macro_rules! copy_paste_menu {
                ($ui: ident) => {{
                    if $ui
                        .button(&*self.translations.c().button_delete_text)
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::Delete, idx));
                    }
                    if $ui
                        .button(&*self.translations.c().button_cut_text)
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::Cut, idx));
                    }
                    if $ui
                        .button(&*self.translations.c().button_copy_text)
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::Copy, idx));
                    }

                    let clipboard_type = match &self.clipboard {
                        Some(NbtClipboard::CompoundEntry(_, _)) => 2,
                        Some(NbtClipboard::ListEntry(_)) => 1,
                        None => 0,
                    };

                    if $ui
                        .add_enabled_ui(clipboard_type != 0, |ui| {
                            ui.button(&*self.translations.c().button_paste_value_text)
                        })
                        .inner
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::ValueInPlace, idx));
                    }

                    if $ui
                        .add_enabled_ui(clipboard_type == 2, |ui| {
                            ui.button(&*self.translations.c().button_paste_key_value_text)
                        })
                        .inner
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::TagAndValueInPlace, idx));
                    }

                    if $ui
                        .add_enabled_ui(clipboard_type != 0, |ui| {
                            ui.button(&*self.translations.c().button_paste_above_text)
                        })
                        .inner
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::InsertAbove, idx));
                    }

                    if $ui
                        .add_enabled_ui(clipboard_type != 0, |ui| {
                            ui.button(&*self.translations.c().button_paste_below_text)
                        })
                        .inner
                        .clicked()
                    {
                        copy_paste = Some((CopyPasteAction::InsertBelow, idx));
                    }
                }};
            }

            macro_rules! simple_view_value {
                ($value: ident, $icon: expr, $th: ident, $ctx_menu: expr) => {{
                    let (_, m_edit, new_value) = self.show_entry(
                        child_id,
                        egui_id,
                        builder,
                        EntryContext {
                            val: Some($value),
                            key: Some(key.to_str().as_ref()),
                            idx: None,
                            extra: None,
                            icon: $icon,
                            type_hint: &*self.translations.c().$th,
                            context_menu: $ctx_menu,
                        },
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if let Some(new_value) = new_value {
                        *$value = new_value;
                    }
                }};
            }

            macro_rules! simple_view_list {
                ($values: ident, $icon: ident, $th: ident, $th_elem: ident, $context_menu: expr) => {{
                    let (open, m_edit, _) = self.show_entry::<()>(
                        child_id.clone(),
                        egui_id,
                        builder,
                        EntryContext {
                            val: None,
                            key: Some(key.to_str().as_ref()),
                            idx: None,
                            extra: Some(&self.translations.f(
                                "list-element-count",
                                &HashMap::from([("count".into(), $values.len().into())]),
                            )),
                            icon: &self.icon_array,
                            type_hint: &*self.translations.c().$th,
                            context_menu: $context_menu,
                        },
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if open {
                        self.show_list(
                            child_id.childs(),
                            egui_id,
                            builder,
                            $values,
                            |tab_viewer, id, egui_id, builder, idx, value| {
                                let (_, _, new_value) = tab_viewer.show_entry(
                                    id,
                                    egui_id,
                                    builder,
                                    EntryContext {
                                        val: Some(value),
                                        key: None,
                                        idx: Some(idx),
                                        extra: None,
                                        icon: &tab_viewer.$icon,
                                        type_hint: &*tab_viewer.translations.c().$th_elem,
                                        context_menu: |ui| {
                                            // TODO: Array element context menu
                                            ui.label("TODO: Array element context menu");
                                        },
                                    },
                                );

                                if let Some(new_value) = new_value {
                                    *value = new_value;
                                }
                            },
                        );
                    }

                    builder.close_dir();
                }};
            }

            macro_rules! convert_to {
                ($ui: ident, $v: ident, $th: ident, $variant: ident, $t: ident) => {{
                    if $ui.button(&*self.translations.c().$th).clicked() {
                        update_type = Some(NbtTag::$variant($v as $t));
                    }
                }};
            }

            macro_rules! convert_to_array {
                ($ui: ident, $v: ident, $th: ident, $variant: ident, $t: ident) => {{
                    if $ui.button(&*self.translations.c().$th).clicked() {
                        update_type = Some(NbtTag::$variant($v.iter().map(|v| *v as $t).collect()));
                    }
                }};
            }

            macro_rules! convert_array_to_list {
                ($ui: ident, $v: ident, $th: ident, $variant: ident, $t: ident) => {{
                    if $ui.button(&*self.translations.c().$th).clicked() {
                        update_type = Some(NbtTag::List(NbtList::$variant(
                            $v.iter().map(|v| *v as $t).collect(),
                        )));
                    }
                }};
            }

            macro_rules! convert_from_string {
                ($ui: ident, $v: expr, $th: ident, $variant: ident, $t: ident) => {{
                    if $ui.button(&*self.translations.c().$th).clicked() {
                        if let Ok(n_value) = $t::from_str(&$v) {
                            update_type = Some(NbtTag::$variant(n_value));
                        }
                    }
                }};
            }

            macro_rules! convert_to_string {
                ($ui: ident, $v: ident) => {
                    if $ui.button(&*self.translations.c().type_hint_str).clicked() {
                        update_type = Some(NbtTag::String(Mutf8String::from($v.to_string())));
                    }
                };
            }

            match tag {
                NbtTag::Byte(b) => {
                    let cb = *b;
                    simple_view_value!(b, &self.icon_numeric, type_hint_i8, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, cb, type_hint_i16, Short, i16);
                                convert_to!(ui, cb, type_hint_i32, Int, i32);
                                convert_to!(ui, cb, type_hint_i64, Long, i64);
                                convert_to!(ui, cb, type_hint_f32, Float, f32);
                                convert_to!(ui, cb, type_hint_f64, Double, f64);
                                convert_to_string!(ui, cb);
                            },
                        );
                    });
                }
                NbtTag::Short(s) => {
                    let cs = *s;
                    simple_view_value!(s, &self.icon_numeric, type_hint_i16, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, cs, type_hint_i8, Byte, i8);
                                convert_to!(ui, cs, type_hint_i32, Int, i32);
                                convert_to!(ui, cs, type_hint_i64, Long, i64);
                                convert_to!(ui, cs, type_hint_f32, Float, f32);
                                convert_to!(ui, cs, type_hint_f64, Double, f64);
                                convert_to_string!(ui, cs);
                            },
                        );
                    });
                }
                NbtTag::Int(i) => {
                    let ci = *i;
                    simple_view_value!(i, &self.icon_numeric, type_hint_i32, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, ci, type_hint_i8, Byte, i8);
                                convert_to!(ui, ci, type_hint_i16, Short, i16);
                                convert_to!(ui, ci, type_hint_i64, Long, i64);
                                convert_to!(ui, ci, type_hint_f32, Float, f32);
                                convert_to!(ui, ci, type_hint_f64, Double, f64);
                                convert_to_string!(ui, ci);
                            },
                        );
                    });
                }
                NbtTag::Long(l) => {
                    let cl = *l;
                    simple_view_value!(l, &self.icon_numeric, type_hint_i64, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, cl, type_hint_i8, Byte, i8);
                                convert_to!(ui, cl, type_hint_i16, Short, i16);
                                convert_to!(ui, cl, type_hint_i32, Int, i32);
                                convert_to!(ui, cl, type_hint_f32, Float, f32);
                                convert_to!(ui, cl, type_hint_f64, Double, f64);
                                convert_to_string!(ui, cl);
                            },
                        );
                    });
                }
                NbtTag::Float(f) => {
                    let cf = *f;
                    simple_view_value!(f, &self.icon_numeric, type_hint_f32, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, cf, type_hint_i8, Byte, i8);
                                convert_to!(ui, cf, type_hint_i16, Short, i16);
                                convert_to!(ui, cf, type_hint_i32, Int, i32);
                                convert_to!(ui, cf, type_hint_i64, Long, i64);
                                convert_to!(ui, cf, type_hint_f64, Double, f64);
                                convert_to_string!(ui, cf);
                            },
                        );
                    });
                }
                NbtTag::Double(d) => {
                    let cd = *d;
                    simple_view_value!(d, &self.icon_numeric, type_hint_f64, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_change_type,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_to!(ui, cd, type_hint_i8, Byte, i8);
                                convert_to!(ui, cd, type_hint_i16, Short, i16);
                                convert_to!(ui, cd, type_hint_i32, Int, i32);
                                convert_to!(ui, cd, type_hint_i64, Long, i64);
                                convert_to!(ui, cd, type_hint_f32, Float, f32);
                                convert_to_string!(ui, cd);
                            },
                        );
                    });
                }
                NbtTag::String(s) => {
                    simple_view_value!(s, &self.icon_string, type_hint_str, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(
                            &*self.translations.c().compound_simple_value_try_parse,
                            |ui| {
                                conv_warn!(self.translations, ui);
                                convert_from_string!(ui, s.to_str(), type_hint_i8, Byte, i8);
                                convert_from_string!(ui, s.to_str(), type_hint_i16, Short, i16);
                                convert_from_string!(ui, s.to_str(), type_hint_i32, Int, i32);
                                convert_from_string!(ui, s.to_str(), type_hint_i64, Long, i64);
                                convert_from_string!(ui, s.to_str(), type_hint_f32, Float, f32);
                                convert_from_string!(ui, s.to_str(), type_hint_f64, Double, f64);
                            },
                        );
                    });
                }

                NbtTag::ByteArray(ba) => {
                    simple_view_list!(ba, icon_numeric, type_hint_byte_array, type_hint_u8, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(&*self.translations.c().compound_array_change_type, |ui| {
                            conv_warn!(self.translations, ui);
                            convert_to_array!(ui, ba, type_hint_int_array, IntArray, i32);
                            convert_to_array!(ui, ba, type_hint_long_array, LongArray, i64);
                            convert_array_to_list!(ui, ba, type_hint_list, Byte, i8);
                        });
                    });
                }
                NbtTag::IntArray(ia) => {
                    simple_view_list!(ia, icon_numeric, type_hint_int_array, type_hint_i32, |ui| {
                        copy_paste_menu!(ui);
                        ui.separator();
                        ui.menu_button(&*self.translations.c().compound_array_change_type, |ui| {
                            conv_warn!(self.translations, ui);
                            convert_to_array!(ui, ia, type_hint_byte_array, ByteArray, u8);
                            convert_to_array!(ui, ia, type_hint_long_array, LongArray, i64);
                            convert_array_to_list!(ui, ia, type_hint_list, Int, i32);
                        });
                    });
                }
                NbtTag::LongArray(la) => {
                    simple_view_list!(
                        la,
                        icon_numeric,
                        type_hint_long_array,
                        type_hint_i64,
                        |ui| {
                            copy_paste_menu!(ui);
                            ui.separator();
                            ui.menu_button(
                                &*self.translations.c().compound_array_change_type,
                                |ui| {
                                    conv_warn!(self.translations, ui);
                                    convert_to_array!(ui, la, type_hint_byte_array, ByteArray, u8);
                                    convert_to_array!(ui, la, type_hint_int_array, IntArray, i32);
                                    convert_array_to_list!(ui, la, type_hint_list, Long, i64);
                                },
                            );
                        }
                    );
                }

                NbtTag::Compound(c) => {
                    let (open, m_edit, _) = self.show_entry::<()>(
                        child_id.clone(),
                        egui_id,
                        builder,
                        EntryContext {
                            val: None,
                            key: Some(key.to_str().as_ref()),
                            idx: None,
                            extra: Some(&self.translations.f(
                                "compound-keys-count",
                                &HashMap::from([("count".into(), c.iter().count().into())]),
                            )),
                            icon: &self.icon_compound_nbt,
                            type_hint: &self.translations.c().type_hint_compound,
                            context_menu: |ui| {
                                copy_paste_menu!(ui);
                            },
                        },
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if open {
                        self.show_compound_tree(c, child_id.childs(), egui_id, builder);
                    }

                    builder.close_dir();
                }

                NbtTag::List(l) => {
                    let list_len = nbt_list_len(l);
                    if matches!(l, NbtList::Empty) {
                        builder.node(
                            NodeBuilder::leaf(child_id)
                                .label_ui(|ui| {
                                    ui.horizontal(|ui| {
                                        Label::new(
                                            RichText::new(self.icon_list.0)
                                                .family(self.icon_list.1.clone())
                                                .color(ui.visuals().text_color()),
                                        )
                                        .sense(Sense::empty())
                                        .selectable(false)
                                        .show_tooltip_when_elided(false)
                                        .ui(ui);

                                        edit = Self::editable_str_label(
                                            ui,
                                            egui_id.with("editable-key"),
                                            key.to_str().as_ref(),
                                            &self.translations.c().editable_key_empty_text,
                                        )
                                        .and_then(|s| {
                                            if s.trim().is_empty() {
                                                None
                                            } else {
                                                Some((idx, s))
                                            }
                                        })
                                        .or(edit.take());

                                        Label::new(":")
                                            .sense(Sense::empty())
                                            .selectable(false)
                                            .show_tooltip_when_elided(false)
                                            .ui(ui);

                                        Label::new(
                                            RichText::new(&*self.translations.c().empty_list_text)
                                                .color(ui.visuals().text_color()),
                                        )
                                        .ui(ui);
                                    })
                                    .response
                                    .interact(Sense::hover())
                                    .on_hover_text(&*self.translations.c().type_hint_list_lists);
                                })
                                .context_menu(|ui| {
                                    copy_paste_menu!(ui);
                                    ui.separator();
                                    if let Some(n_value) = self
                                        .show_nbt_list_entry_context_menu_type_conversion(
                                            ui, l, true,
                                        )
                                    {
                                        update_type = Some(n_value);
                                    }
                                }),
                        );
                    } else {
                        let (open, m_edit, _) = self.show_entry::<()>(
                            child_id.clone(),
                            egui_id,
                            builder,
                            EntryContext {
                                val: None,
                                key: Some(key.to_str().as_ref()),
                                idx: None,
                                extra: Some(&self.translations.f(
                                    "list-element-count",
                                    &HashMap::from([("count".into(), list_len.into())]),
                                )),
                                icon: &self.icon_list,
                                type_hint: nbt_list_type_hint(l, &self.translations),
                                context_menu: |ui| {
                                    copy_paste_menu!(ui);
                                    ui.separator();
                                    if let Some(n_value) = self
                                        .show_nbt_list_entry_context_menu_type_conversion(
                                            ui, l, true,
                                        )
                                    {
                                        update_type = Some(n_value);
                                    }
                                },
                            },
                        );

                        edit = m_edit.map(|s| (idx, s)).or(edit);

                        if open {
                            self.show_nbt_list(l, child_id.childs(), egui_id, builder);
                        }

                        builder.close_dir();
                    }
                }
            }

            if let Some(update_type) = update_type {
                *tag = update_type;
            }
        }

        match copy_paste {
            None => {}
            Some((CopyPasteAction::Delete, idx)) => {
                (**nbt).remove(idx);
            }
            Some((CopyPasteAction::Cut, idx)) => {
                let (k, v) = (**nbt).remove(idx);
                self.clipboard = Some(NbtClipboard::CompoundEntry(k, v));
            }
            Some((CopyPasteAction::Copy, idx)) => {
                if let Some((k, v)) = (**nbt).get(idx) {
                    self.clipboard = Some(NbtClipboard::CompoundEntry(k.clone(), v.clone()));
                }
            }
            Some((CopyPasteAction::ValueInPlace, idx)) => match &self.clipboard {
                None => {}
                Some(NbtClipboard::CompoundEntry(_, tag)) | Some(NbtClipboard::ListEntry(tag)) => {
                    if let Some((_, v)) = (**nbt).get_mut(idx) {
                        *v = tag.clone();
                    }
                }
            },
            Some((CopyPasteAction::TagAndValueInPlace, idx)) => match &self.clipboard {
                None => {}
                Some(NbtClipboard::ListEntry(_)) => {}
                Some(NbtClipboard::CompoundEntry(key, tag)) => {
                    if let Some(e) = (**nbt).get_mut(idx) {
                        *e = (key.clone(), tag.clone());
                    }
                }
            },
            Some((CopyPasteAction::InsertAbove, idx)) => match &self.clipboard {
                None => {}
                Some(NbtClipboard::ListEntry(tag)) => {
                    (**nbt).insert(idx, ("".into(), tag.clone()));
                }
                Some(NbtClipboard::CompoundEntry(key, tag)) => {
                    (**nbt).insert(idx, (key.clone(), tag.clone()));
                }
            },
            Some((CopyPasteAction::InsertBelow, idx)) => match &self.clipboard {
                None => {}
                Some(NbtClipboard::ListEntry(tag)) => {
                    (**nbt).insert(idx + 1, ("".into(), tag.clone()));
                }
                Some(NbtClipboard::CompoundEntry(key, tag)) => {
                    (**nbt).insert(idx + 1, (key.clone(), tag.clone()));
                }
            },
        }

        if let Some((idx, new_key)) = edit
            && let Some(k) = nbt.keys_mut().nth(idx)
        {
            *k = new_key.into();
        }
    }
}

impl TabViewer for NbtTabViewer {
    type Tab = NbtDocumentTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        if *self.per_short_title.get(&tab.title_short).unwrap_or(&0) > 1 {
            (&tab.title_long).into()
        } else {
            (&tab.title_short).into()
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match &mut tab.data {
            DocumentData::Loading => {
                ui.horizontal(|ui| {
                    Spinner::new().ui(ui);
                    ui.label("Loading");
                });
            }
            DocumentData::Saving => {
                ui.horizontal(|ui| {
                    Spinner::new().ui(ui);
                    ui.label("Saving");
                });
            }
            DocumentData::ReadError(e) => {
                ui.label(format!("{:?}", *e));
            }
            DocumentData::Nbt(nbt, compression) => {
                ui.horizontal(|ui| {
                    ui.label("Compression:");
                    egui::ComboBox::from_id_salt("compression")
                        .selected_text(format!("{:?}", *compression))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(compression, NbtCompression::None, "None")
                                .changed()
                                | ui.selectable_value(compression, NbtCompression::Gzip, "Gzip")
                                    .changed()
                                | ui.selectable_value(compression, NbtCompression::Zlib, "Zlib")
                                    .changed()
                            {
                                tab.modified = true;
                            }
                        });
                });
                ui.separator();
                let tree_rect = ui.available_rect_before_wrap();
                ui.allocate_ui(tree_rect.size(), |ui| {
                    TreeView::new("nbt_tree".into()).show(ui, |builder| {
                        self.show_nbt_tree(nbt, tab.root_id, builder);
                    });
                });
            }
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if let Some(count) = self.per_short_title.get_mut(&tab.title_short) {
            let (ncount, overflow) = count.overflowing_sub(1);
            if overflow {
                OnCloseResponse::Close
            } else if ncount == 0 {
                self.per_short_title.remove_entry(&tab.title_short);
                OnCloseResponse::Close
            } else {
                *count = ncount;
                OnCloseResponse::Close
            }
        } else {
            OnCloseResponse::Close
        }
    }
}
