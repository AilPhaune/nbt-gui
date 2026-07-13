use anyhow::Context;
use eframe::{App, Frame};
use egui::{MenuBar, Modal, ScrollArea, Slider, Ui, ViewportCommand, Widget};
use egui_dock::{DockArea, DockState};
use futures::SinkExt;
use rfd::AsyncFileDialog;
use smallvec::{SmallVec, smallvec};
use std::{collections::HashMap, sync::Arc};
use tokio::runtime::Runtime;

use crate::{
    i18n::Translations,
    ui::{
        document::{DocumentData, NbtDocumentTab},
        tabs::{NbtTabViewer, TabEvent},
    },
};

pub struct NbtEditorApplication {
    translations: Arc<Translations>,
    tab_viewer: NbtTabViewer,
    dock_state: DockState<NbtDocumentTab>,
    errors: Vec<anyhow::Error>,
    runtime: Runtime,
    scale: f64,
    scale_slider: u32,
}

impl NbtEditorApplication {
    pub fn new(translations: Arc<Translations>) -> anyhow::Result<Self> {
        Ok(Self {
            tab_viewer: NbtTabViewer::new(Arc::clone(&translations))?,
            translations,
            dock_state: DockState::new(vec![]),
            errors: vec![],
            runtime: Runtime::new().context("Failed to create tokio runtime")?,
            scale: 1.5,
            scale_slider: 150,
        })
    }

    fn open_files(&self) {
        let mut event_tx = self.tab_viewer.events_tx.clone();
        self.runtime.spawn(async move {
            if let Some(handles) = AsyncFileDialog::new()
                .add_filter("Compressed/uncompressed NBT data", &["nbt", "dat"])
                .add_filter("MCA Files (Anvil)", &["mca"])
                .pick_files()
                .await
            {
                for handle in handles {
                    // only possible errors are `Full` (doesn't happen because the channel is unbounded) and `Disconnected` (in which case we don't care)
                    let _ = event_tx
                        .send(TabEvent::OpenNewFileTab {
                            path: handle.path().into(),
                        })
                        .await;
                }
            }
        });
    }

    fn open_test_nbt(&mut self) {
        let mut tab =
            NbtDocumentTab::new_titled(String::clone(&self.translations.t("title-test-nbt")));
        tab.data = DocumentData::example_nbt();
        if self.tab_viewer.insert(&mut tab) {
            self.dock_state.push_to_focused_leaf(tab);
        }
    }

    fn new_untitled_file(&self) {
        let mut event_tx = self.tab_viewer.events_tx.clone();
        self.runtime.spawn(async move {
            // only possible errors are `Full` (doesn't happen because the channel is unbounded) and `Disconnected` (in which case we don't care)
            let _ = event_tx.send(TabEvent::OpenNewTab).await;
        });
    }

    fn find_tab(&mut self, tab_id: usize) -> Option<&mut NbtDocumentTab> {
        self.dock_state
            .find_tab_from(|tab| tab.tab_id == tab_id)
            .and_then(|p| {
                self.dock_state[p.node_path()]
                    .tabs_mut()
                    .and_then(|t| t.get_mut(p.tab.0))
            })
    }
}

impl App for NbtEditorApplication {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        MenuBar::new().ui(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button(&*self.translations.t("menu-file"), |ui| {
                    if ui.button(&*self.translations.t("menu-file-new")).clicked() {
                        self.new_untitled_file();
                    }

                    ui.separator();

                    if ui
                        .button(&*self.translations.t("menu-file-open-test-nbt"))
                        .clicked()
                    {
                        self.open_test_nbt();
                    }

                    if ui.button(&*self.translations.t("menu-file-open")).clicked() {
                        self.open_files();
                    }

                    ui.separator();

                    if ui.button(&*self.translations.t("menu-file-save")).clicked() {
                        if let Some((_rect, tab)) = self.dock_state.find_active_focused() {
                            let tab_id = tab.tab_id;
                            let save = tab.action_save();
                            let mut event_tx = self.tab_viewer.events_tx.clone();
                            self.runtime.spawn(async move {
                                let (path, data, result) = save.await;
                                // only possible errors are `Full` (doesn't happen because the channel is unbounded) and `Disconnected` (in which case we don't care)
                                let _ = event_tx
                                    .send(TabEvent::SaveFileTabResult {
                                        tab_id,
                                        path,
                                        data,
                                        result,
                                    })
                                    .await;
                            });
                        }
                    }

                    if ui
                        .button(&*self.translations.t("menu-file-save-as"))
                        .clicked()
                    {
                        if let Some((_rect, tab)) = self.dock_state.find_active_focused() {
                            let tab_id = tab.tab_id;
                            let save = tab.action_save_as();
                            let mut event_tx = self.tab_viewer.events_tx.clone();
                            self.runtime.spawn(async move {
                                let (path, data, result) = save.await;
                                // only possible errors are `Full` (doesn't happen because the channel is unbounded) and `Disconnected` (in which case we don't care)
                                let _ = event_tx
                                    .send(TabEvent::SaveFileTabResult {
                                        tab_id,
                                        path,
                                        data,
                                        result,
                                    })
                                    .await;
                            });
                        }
                    }

                    if ui.button(&*self.translations.t("menu-file-exit")).clicked() {
                        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    }
                })
            });

            ui.menu_button(&*self.translations.t("menu-preferences"), |ui| {
                ui.menu_button(&*self.translations.t("menu-preferences-zoom"), |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        for size in [25, 33, 50, 75, 100, 125, 150, 175, 200, 300, 400, 500] {
                            if ui
                                .button(
                                    &*self.translations.f(
                                        "percentage",
                                        &HashMap::from([("p".into(), size.into())]),
                                    ),
                                )
                                .clicked()
                            {
                                self.scale = (size as f64) * 0.01;
                                self.scale_slider = (self.scale * 100.0).round() as u32;
                            }
                        }

                        ui.horizontal(|ui| {
                            Slider::new(&mut self.scale_slider, 10..=500)
                                .custom_formatter(|v, _| {
                                    self.translations
                                        .f("percentage", &HashMap::from([("p".into(), v.into())]))
                                })
                                .integer()
                                .ui(ui);
                            if ui
                                .button(&*self.translations.t("button-confirm-text"))
                                .clicked()
                            {
                                self.scale = (self.scale_slider as f64) * 0.01;
                            }
                        });
                    });
                });
            });
        });

        let mut rem: SmallVec<[usize; 2]> = smallvec![];
        for (i, err) in self.errors.iter().enumerate() {
            let mut close = false;

            Modal::new(format!("error_dialog_{i}").into()).show(ui, |ui| {
                ui.heading(&*self.translations.t("dialog-error"));
                ui.separator();

                ui.label(format!("{:?}", err));

                if ui.button("OK").clicked() {
                    close = true;
                }
            });

            if close {
                rem.push(i);
            }
        }
        for &i in rem.iter().rev() {
            self.errors.swap_remove(i);
        }

        DockArea::new(&mut self.dock_state).show_inside(ui, &mut self.tab_viewer);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        ctx.set_pixels_per_point(self.scale as f32);

        while let Ok(event) = self.tab_viewer.events_rx.try_recv() {
            match event {
                TabEvent::OpenNewTab => {
                    let mut tab = NbtDocumentTab::new_titled(String::clone(
                        &self.translations.t("title-untitled"),
                    ));
                    if self.tab_viewer.insert(&mut tab) {
                        self.dock_state.push_to_focused_leaf(tab);
                    }
                }
                TabEvent::OpenNewFileTab { path } => {
                    let mut tab = NbtDocumentTab::new(path);
                    if self.tab_viewer.insert(&mut tab) {
                        let mut event_tx = self.tab_viewer.events_tx.clone();
                        let load = tab.action_load();
                        self.runtime.spawn(async move {
                            let (tab_id, data) = load.await;
                            // only possible errors are `Full` (doesn't happen because the channel is unbounded) and `Disconnected` (in which case we don't care)
                            let _ = event_tx
                                .send(TabEvent::DoneOpeningNewTabFile { tab_id, data })
                                .await;
                        });
                        self.dock_state.push_to_focused_leaf(tab);
                    }
                }
                TabEvent::SaveFileTabResult {
                    tab_id,
                    path,
                    data,
                    result,
                } => {
                    self.find_tab(tab_id).map(|tab| {
                        if let Some(path) = path {
                            tab.saved_location = Some(path);
                        }
                        tab.data = data;
                    });
                    if let Err(e) = result {
                        self.errors.push(e);
                    }
                }
                TabEvent::DoneOpeningNewTabFile { tab_id, data } => {
                    self.find_tab(tab_id).map(|tab| {
                        tab.data = match data {
                            Ok(d) => d,
                            Err(e) => DocumentData::ReadError(Arc::new(e)),
                        }
                    });
                }
            }
        }
    }

    fn on_exit(&mut self) {}
}
