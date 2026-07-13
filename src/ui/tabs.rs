use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, anyhow};
use egui::{FontFamily, Id, Key, Label, RichText, Sense, Spinner, Stroke, TextEdit, Ui, Widget};
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
    fn from_str(s: String) -> Result<Self, ()>;
    fn to_str(&self) -> String;
}

impl NbtValue for Mutf8String {
    fn from_str(s: String) -> Result<Self, ()> {
        Ok(Mutf8String::from_string(s))
    }

    fn to_str(&self) -> String {
        self.to_string()
    }
}

impl NbtValue for () {
    fn from_str(_: String) -> Result<Self, ()> {
        Ok(())
    }

    fn to_str(&self) -> String {
        String::new()
    }
}

macro_rules! impl_fromstr2 {
    ($($t:ty),*) => {
        $(impl NbtValue for $t {
            fn from_str(s: String) -> Result<Self, ()> {
                s.parse().map_err(|_| ())
            }

            fn to_str(&self) -> String {
                self.to_string()
            }
        })*
    };
}

impl_fromstr2!(i8, u8, i16, i32, i64, f32, f64, bool);

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
}

type NbtNodeId = Vec<u32>;

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

fn nbt_list_type_hint(list: &NbtList) -> &'static str {
    match list {
        NbtList::Byte(_) => "type-hint-list-i8",
        NbtList::ByteArray(_) => "type-hint-list-byte-arrays",
        NbtList::Compound(_) => "type-hint-list-compounds",
        NbtList::Double(_) => "type-hint-list-f64",
        NbtList::Empty => "type-hint-empty-list",
        NbtList::Float(_) => "type-hint-list-f32",
        NbtList::Int(_) => "type-hint-list-i32",
        NbtList::IntArray(_) => "type-hint-list-i32-arrays",
        NbtList::List(_) => "type-hint-list-lists",
        NbtList::Long(_) => "type-hint-list-i64",
        NbtList::LongArray(_) => "type-hint-list-i64-array",
        NbtList::Short(_) => "type-hint-list-i16",
        NbtList::String(_) => "type-hint-list-strs",
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

    fn editable_str_label(
        ui: &mut egui::Ui,
        id: egui::Id,
        current: &str,
        text_empty: &str,
    ) -> Option<String> {
        let editing_id = egui::Id::new("nbt_tree_currently_editing");
        let buffer_id = id.with("buf");

        let is_editing = ui.memory(|m| m.data.get_temp::<Id>(editing_id)) == Some(id);

        let just_done_editing_id = id.with("just_done");

        if !is_editing {
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

            if response.clicked() {
                ui.memory_mut(|m| {
                    m.data.insert_temp(buffer_id, current.to_string());
                    m.data.insert_temp(editing_id, id);
                });
                ui.ctx().request_repaint();
            }
            return None;
        }

        let mut buffer = ui
            .memory_mut(|m| m.data.get_temp::<String>(buffer_id))
            .unwrap_or_else(|| current.to_string());

        let text_edit_id = id.with("text_edit");
        let just_started_editing_id = id.with("just_started");

        let response = ui.add(
            TextEdit::singleline(&mut buffer)
                .id(text_edit_id)
                .hint_text(text_empty),
        );

        let already_requested = ui
            .memory(|m| m.data.get_temp::<bool>(just_started_editing_id))
            .unwrap_or(false);

        if !already_requested {
            response.request_focus();
            ui.memory_mut(|m| m.data.insert_temp(just_started_editing_id, true));
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
                &*self.translations.t("unnamed-root-nbt-text-hint"),
            ) {
                let old = std::mem::replace(nbt, BaseNbt::default());
                let tag = old.as_compound();
                *nbt = BaseNbt::new(Mutf8String::from_string(new_name), tag);
            }
        });
    }

    fn show_nbt_tree(
        &mut self,
        nbt: &mut Nbt,
        id: &mut NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) {
        match nbt {
            Nbt::None => {
                builder.node(
                    NodeBuilder::leaf(id.clone())
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

                                Label::new(&*self.translations.t("root-nbt-empty-text"))
                                    .sense(Sense::empty())
                                    .selectable(false)
                                    .ui(ui);
                            });
                        })
                        .context_menu(|ui| {
                            if ui
                                .button(&*self.translations.t("button-create-empty-root"))
                                .clicked()
                            {
                                *nbt = Nbt::Some(BaseNbt::new("", NbtCompound::new()));
                            }
                        }),
                );
            }
            Nbt::Some(bnbt) => {
                let mut delete_requested = false;

                let dir_open = builder.node(
                    NodeBuilder::dir(id.clone())
                        .label_ui(|ui| {
                            self.build_base_nbt_label(ui, egui_id, bnbt);
                        })
                        .context_menu(|ui| {
                            if ui
                                .button(&*self.translations.t("button-delete-text"))
                                .clicked()
                            {
                                delete_requested = true;
                            }
                        })
                        .default_open(false),
                );

                if dir_open {
                    id.push(0);
                    self.show_compound_tree(&mut *bnbt, id, egui_id.with(0), builder);
                }

                builder.close_dir();

                if delete_requested {
                    *nbt = Nbt::None;
                }
            }
        }
    }

    fn show_entry<T: NbtValue>(
        &self,
        id: &NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
        mut val: Option<&mut T>,
        key: Option<&str>,
        idx: Option<usize>,
        extra: Option<&str>,
        icon: &(char, FontFamily),
        type_hint: &str,
    ) -> (bool, Option<String>) {
        let mut ret = None;

        let node_base = match val {
            Some(_) => NodeBuilder::leaf(id.clone()),
            None => NodeBuilder::dir(id.clone()).default_open(false),
        };

        let open = builder.node(node_base.label_ui(|ui| {
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
                        &*self.translations.t("editable-key-empty-text"),
                    )
                    .and_then(|s| if s.trim().is_empty() { None } else { Some(s) })
                }

                if let Some(idx) = idx {
                    Label::new(RichText::new(idx.to_string()).color(ui.visuals().text_color()))
                        .sense(Sense::empty())
                        .show_tooltip_when_elided(false)
                        .ui(ui);
                }

                if let Some(val) = &mut val {
                    Label::new(RichText::new(":").color(ui.visuals().text_color()))
                        .sense(Sense::empty())
                        .selectable(false)
                        .show_tooltip_when_elided(false)
                        .ui(ui);

                    if let Some(new_val) = Self::editable_str_label(
                        ui,
                        egui_id.with("editable-value"),
                        &val.to_str(),
                        &*self.translations.t("editable-value-empty-text"),
                    ) && let Ok(parsed) = T::from_str(new_val)
                    {
                        **val = parsed;
                    }
                }

                if let Some(extra) = extra {
                    Label::new(RichText::new(extra).color(ui.visuals().text_color())).ui(ui);
                }
            })
            .response
            .interact(Sense::hover())
            .on_hover_text(type_hint);
        }));

        (open, ret)
    }

    fn show_list<T>(
        &self,
        id: &mut NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
        values: &mut [T],
        renderer: impl Fn(&mut NbtNodeId, Id, &mut TreeViewBuilder<NbtNodeId>, usize, &mut T),
    ) {
        for (idx, value) in values.iter_mut().enumerate() {
            id.push(idx as u32);
            let egui_id = egui_id.with(idx);

            renderer(id, egui_id, builder, idx, value);

            id.pop();
        }
    }

    fn show_nbt_list(
        &self,
        nbt: &mut NbtList,
        id: &mut NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) {
        macro_rules! simple_view_list {
            ($values: ident, $icon: expr, $th: expr) => {
                self.show_list(
                    id,
                    egui_id,
                    builder,
                    $values,
                    |id, egui_id, builder, idx, value| {
                        self.show_entry(
                            id,
                            egui_id,
                            builder,
                            Some(value),
                            None,
                            Some(idx),
                            None,
                            $icon,
                            &*self.translations.t($th),
                        );
                    },
                )
            };
        }

        macro_rules! simple_view_list_list {
            ($values: ident, $th: expr, $th_elem: expr) => {{
                self.show_list(
                    id,
                    egui_id,
                    builder,
                    $values,
                    |id, egui_id, builder, idx, value| {
                        let (open, _) = self.show_entry::<()>(
                            id,
                            egui_id,
                            builder,
                            None,
                            None,
                            Some(idx),
                            Some(&self.translations.f(
                                "list-element-count",
                                &HashMap::from([("count".into(), value.len().into())]),
                            )),
                            &self.icon_array,
                            &*self.translations.t($th),
                        );

                        if open {
                            self.show_list(
                                id,
                                egui_id,
                                builder,
                                value,
                                |id, egui_id, builder, idx, value| {
                                    self.show_entry(
                                        id,
                                        egui_id,
                                        builder,
                                        Some(value),
                                        None,
                                        Some(idx),
                                        None,
                                        &self.icon_numeric,
                                        &*self.translations.t($th_elem),
                                    );
                                },
                            )
                        }

                        builder.close_dir();
                    },
                );
            }};
        }

        match nbt {
            NbtList::Empty => unreachable!("This is a bug!"),
            NbtList::Byte(bs) => simple_view_list!(bs, &self.icon_numeric, "type-hint-i8"),
            NbtList::Short(shs) => simple_view_list!(shs, &self.icon_numeric, "type-hint-i16"),
            NbtList::Int(is) => simple_view_list!(is, &self.icon_numeric, "type-hint-i32"),
            NbtList::Long(ls) => simple_view_list!(ls, &self.icon_numeric, "type-hint-i64"),
            NbtList::Float(fs) => simple_view_list!(fs, &self.icon_numeric, "type-hint-f32"),
            NbtList::Double(ds) => simple_view_list!(ds, &self.icon_numeric, "type-hint-f64"),
            NbtList::String(strs) => simple_view_list!(strs, &self.icon_string, "type-hint-str"),
            NbtList::ByteArray(bas) => {
                simple_view_list_list!(bas, "type-hint-byte-array", "type-hint-u8")
            }
            NbtList::IntArray(ias) => {
                simple_view_list_list!(ias, "type-hint-int-array", "type-hint-i32")
            }
            NbtList::LongArray(las) => {
                simple_view_list_list!(las, "type-hint-long-array", "type-hint-i64")
            }
            NbtList::List(ls) => self.show_list(
                id,
                egui_id,
                builder,
                ls,
                |id, egui_id, builder, idx, value| {
                    let list_len = nbt_list_len(value);

                    if matches!(value, NbtList::Empty) {
                        builder.node(NodeBuilder::leaf(id.clone()).label_ui(|ui| {
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

                                ui.label(
                                    RichText::new(idx.to_string()).color(ui.visuals().text_color()),
                                );

                                Label::new(":")
                                    .sense(Sense::empty())
                                    .selectable(false)
                                    .show_tooltip_when_elided(false)
                                    .ui(ui);

                                Label::new(
                                    RichText::new(&*self.translations.t("empty-list-text"))
                                        .color(ui.visuals().text_color()),
                                )
                                .ui(ui);
                            })
                            .response
                            .interact(Sense::hover())
                            .on_hover_text(&*self.translations.t(nbt_list_type_hint(value)));
                        }));

                        return;
                    }

                    let (open, _) = self.show_entry::<()>(
                        id,
                        egui_id,
                        builder,
                        None,
                        None,
                        Some(idx),
                        Some(&self.translations.f(
                            "list-element-count",
                            &HashMap::from([("count".into(), list_len.into())]),
                        )),
                        &self.icon_list,
                        &*self.translations.t(nbt_list_type_hint(value)),
                    );

                    if open {
                        self.show_nbt_list(value, id, egui_id, builder);
                    }

                    builder.close_dir();
                },
            ),
            NbtList::Compound(cs) => self.show_list(
                id,
                egui_id,
                builder,
                cs,
                |id, egui_id, builder, idx, value| {
                    let (open, _) = self.show_entry::<()>(
                        id,
                        egui_id,
                        builder,
                        None,
                        None,
                        Some(idx),
                        Some(&self.translations.f(
                            "compound-keys-count",
                            &HashMap::from([("count".into(), value.iter().count().into())]),
                        )),
                        &self.icon_compound_nbt,
                        &*self.translations.t("type-hint-compound"),
                    );

                    if open {
                        self.show_compound_tree(value, id, egui_id, builder);
                    }

                    builder.close_dir();
                },
            ),
        }
    }

    fn show_compound_tree(
        &self,
        nbt: &mut NbtCompound,
        id: &mut NbtNodeId,
        egui_id: Id,
        builder: &mut TreeViewBuilder<NbtNodeId>,
    ) {
        let mut edit = None;
        for (idx, (key, tag)) in nbt.iter_mut().enumerate() {
            id.push(idx as u32);
            let egui_id = egui_id.with(idx);

            macro_rules! simple_view_value {
                ($value: ident, $icon: expr, $th: expr) => {{
                    edit = self
                        .show_entry(
                            id,
                            egui_id,
                            builder,
                            Some($value),
                            Some(key.to_str().as_ref()),
                            None,
                            None,
                            $icon,
                            &*self.translations.t($th),
                        )
                        .1
                        .map(|s| (idx, s))
                        .or(edit);
                }};
            }

            macro_rules! simple_view_list {
                ($values: ident, $icon: expr, $th: expr, $th_elem: expr) => {{
                    let (open, m_edit) = self.show_entry::<()>(
                        id,
                        egui_id,
                        builder,
                        None,
                        Some(key.to_str().as_ref()),
                        None,
                        Some(&self.translations.f(
                            "list-element-count",
                            &HashMap::from([("count".into(), $values.len().into())]),
                        )),
                        &self.icon_array,
                        &*self.translations.t($th),
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if open {
                        self.show_list(
                            id,
                            egui_id,
                            builder,
                            $values,
                            |id, egui_id, builder, idx, value| {
                                self.show_entry(
                                    id,
                                    egui_id,
                                    builder,
                                    Some(value),
                                    None,
                                    Some(idx),
                                    None,
                                    $icon,
                                    &*self.translations.t($th_elem),
                                );
                            },
                        );
                    }

                    builder.close_dir();
                }};
            }

            match tag {
                NbtTag::Byte(b) => simple_view_value!(b, &self.icon_numeric, "type-hint-i8"),
                NbtTag::Short(s) => simple_view_value!(s, &self.icon_numeric, "type-hint-i16"),
                NbtTag::Int(i) => simple_view_value!(i, &self.icon_numeric, "type-hint-i32"),
                NbtTag::Long(l) => simple_view_value!(l, &self.icon_numeric, "type-hint-i64"),
                NbtTag::Float(f) => simple_view_value!(f, &self.icon_numeric, "type-hint-f32"),
                NbtTag::Double(d) => simple_view_value!(d, &self.icon_numeric, "type-hint-f64"),
                NbtTag::String(s) => simple_view_value!(s, &self.icon_string, "type-hint-str"),

                NbtTag::ByteArray(ba) => {
                    simple_view_list!(
                        ba,
                        &self.icon_numeric,
                        "type-hint-byte-array",
                        "type-hint-u8"
                    )
                }
                NbtTag::IntArray(ia) => {
                    simple_view_list!(
                        ia,
                        &self.icon_numeric,
                        "type-hint-int-array",
                        "type-hint-i32"
                    )
                }
                NbtTag::LongArray(la) => {
                    simple_view_list!(
                        la,
                        &self.icon_numeric,
                        "type-hint-long-array",
                        "type-hint-i64"
                    )
                }

                NbtTag::Compound(c) => {
                    let (open, m_edit) = self.show_entry::<()>(
                        id,
                        egui_id,
                        builder,
                        None,
                        Some(key.to_str().as_ref()),
                        None,
                        Some(&self.translations.f(
                            "compound-keys-count",
                            &HashMap::from([("count".into(), c.iter().count().into())]),
                        )),
                        &self.icon_compound_nbt,
                        &*self.translations.t("type-hint-compound"),
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if open {
                        self.show_compound_tree(c, id, egui_id, builder);
                    }

                    builder.close_dir();
                }

                NbtTag::List(l) => {
                    let list_len = nbt_list_len(l);
                    if matches!(l, NbtList::Empty) {
                        builder.node(NodeBuilder::leaf(id.clone()).label_ui(|ui| {
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
                                    &*self.translations.t("editable-key-empty-text"),
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
                                    RichText::new(&*self.translations.t("empty-list-text"))
                                        .color(ui.visuals().text_color()),
                                )
                                .ui(ui);
                            })
                            .response
                            .interact(Sense::hover())
                            .on_hover_text(&*self.translations.t("type-hint-list-lists"));
                        }));

                        continue;
                    }

                    let (open, m_edit) = self.show_entry::<()>(
                        id,
                        egui_id,
                        builder,
                        None,
                        Some(key.to_str().as_ref()),
                        None,
                        Some(&self.translations.f(
                            "list-element-count",
                            &HashMap::from([("count".into(), list_len.into())]),
                        )),
                        &self.icon_list,
                        &*self.translations.t(nbt_list_type_hint(l)),
                    );

                    edit = m_edit.map(|s| (idx, s)).or(edit);

                    if open {
                        self.show_nbt_list(l, id, egui_id, builder);
                    }

                    builder.close_dir();
                }
            }

            id.pop();
        }

        if let Some((idx, new_key)) = edit {
            if !nbt.contains(&new_key) {
                nbt.keys_mut().nth(idx).map(|k| {
                    *k = new_key.into();
                });
            }
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
                        self.show_nbt_tree(nbt, &mut Vec::new(), tab.root_id, builder);
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
