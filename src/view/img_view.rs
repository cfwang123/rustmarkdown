//! 图片文件只读预览（对齐 docview ImageViewer：滚轮缩放、拖拽平移、旋转）。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use egui::epaint::Vertex;
use egui::{
    pos2, Color32, ColorImage, Context, CursorIcon, Key, Mesh, Modifiers, Rect, RichText, Sense,
    Shape, TextureOptions, Ui, Vec2,
};

use crate::io::imgcache::Raster;

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;
const WHEEL: f32 = 1.15;
const BG: Color32 = Color32::from_rgb(0xE5, 0xE7, 0xEB);

pub struct ImageSession {
    pub path: PathBuf,
    pub raster: Option<Raster>,
    pub err: Option<String>,
    pub zoom: f32,
    pub rot_quarter: u8,
    pan: Vec2,
    fitted: bool,
    rx: Option<Receiver<Result<(u32, u32, Vec<u8>), String>>>,
}

impl ImageSession {
    pub fn open(path: &Path) -> Self {
        let path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();
        let p2 = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load_rgba(&p2));
        });
        Self {
            path,
            raster: None,
            err: None,
            zoom: 1.0,
            rot_quarter: 0,
            pan: Vec2::ZERO,
            fitted: false,
            rx: Some(rx),
        }
    }

    pub fn status_text(&self) -> String {
        let tag = crate::io::file::ext_lower(&self.path)
            .map(|e| e.to_ascii_uppercase())
            .unwrap_or_else(|| "IMG".into());
        let dim = self
            .raster
            .as_ref()
            .map(|r| {
                let w = r.size.x.round() as i32;
                let h = r.size.y.round() as i32;
                format!("  ·  {w}×{h}")
            })
            .unwrap_or_default();
        let rot = if self.rot_quarter != 0 {
            format!("  ·  {}°", self.rot_quarter as i32 * 90)
        } else {
            String::new()
        };
        format!("{tag}{dim}{rot}  ·  {:.0}%", self.zoom * 100.0)
    }

    fn disp_px(&self) -> Vec2 {
        let Some(r) = &self.raster else {
            return Vec2::ZERO;
        };
        if self.rot_quarter % 2 == 1 {
            Vec2::new(r.size.y, r.size.x)
        } else {
            r.size
        }
    }

    fn poll(&mut self, ctx: &Context) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        if let Ok(res) = rx.try_recv() {
            self.rx = None;
            match res {
                Ok((w, h, px)) => {
                    let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                    let tex = ctx.load_texture(
                        format!("img-file-{}", self.path.display()),
                        img,
                        TextureOptions::LINEAR,
                    );
                    self.raster = Some(Raster {
                        tex,
                        size: Vec2::new(w as f32, h as f32),
                        rgba: Arc::new(px),
                        local_path: Some(self.path.clone()),
                    });
                    self.fitted = false;
                }
                Err(e) => self.err = Some(e),
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint();
        }
    }
}

pub enum ImgAction {
    None,
    Copy,
    CopyFile,
}

pub fn show(ui: &mut Ui, st: &mut ImageSession) -> ImgAction {
    st.poll(ui.ctx());
    let mut ui = crate::view::pane_ui(ui);
    ui.painter().rect_filled(ui.max_rect(), 0.0, BG);
    if let Some(err) = &st.err {
        ui.add_space(24.0);
        ui.label(RichText::new(crate::i18n::cannot_open_image(err)).color(Color32::from_rgb(0xB9, 0x1C, 0x1C)));
        return ImgAction::None;
    }
    let Some(raster) = st.raster.clone() else {
        ui.add_space(24.0);
        ui.label(RichText::new(crate::i18n::t().opening_image).color(Color32::from_rgb(0x6B, 0x72, 0x80)));
        return ImgAction::None;
    };

    let stage = ui.max_rect();
    if !st.fitted {
        let _ = fit_to_view(st, stage);
    }

    let mut action = ImgAction::None;
    if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::C) || i.consume_key(Modifiers::CTRL, Key::C))
    {
        action = ImgAction::Copy;
    }
    if !ui.ctx().wants_keyboard_input() {
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::OpenBracket)) {
            rotate(st, -1);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::CloseBracket)) {
            rotate(st, 1);
        }
    }

    let mut z = st.zoom;
    if crate::view::ctrl_zoom(&mut ui, &mut z) {
        st.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    let resp = ui.allocate_rect(stage, Sense::click_and_drag());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(if resp.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }

    let pointer = ui.input(|i| i.pointer.interact_pos());
    let shift = ui.input(|i| i.modifiers.shift);
    let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
    if resp.hovered() && scroll_y.abs() > 0.1 {
        ui.input_mut(|i| {
            i.raw_scroll_delta = Vec2::ZERO;
            i.smooth_scroll_delta = Vec2::ZERO;
        });
        if shift {
            st.pan.x += if scroll_y > 0.0 { 80.0 } else { -80.0 };
        } else {
            let f = if scroll_y > 0.0 { WHEEL } else { 1.0 / WHEEL };
            zoom_at(st, st.zoom * f, pointer, stage);
        }
    }

    if resp.double_clicked() {
        if let Some(fit) = fit_zoom(st, stage) {
            if (st.zoom - fit).abs() / fit.max(0.01) < 0.08 {
                st.zoom = 1.0;
            } else {
                st.zoom = fit;
            }
            st.pan = Vec2::ZERO;
        }
    } else if resp.dragged() {
        st.pan += resp.drag_delta();
    }

    let nav = crate::view::consume_key_nav(&mut ui);
    match nav {
        crate::view::KeyNav::Line(d) => st.pan += d,
        crate::view::KeyNav::Page(dir) => {
            st.pan.y -= dir as f32 * (stage.height() * 0.9).max(80.0);
        }
        crate::view::KeyNav::None => {}
    }

    let disp = st.disp_px() * st.zoom;
    let img_rect = Rect::from_center_size(stage.center() + st.pan, disp);
    paint_rot(&mut ui, raster.tex.id(), img_rect, st.rot_quarter, stage);

    resp.context_menu(|ui| {
        if ui.button(crate::i18n::t().copy_image).on_hover_text("Ctrl+C").clicked() {
            action = ImgAction::Copy;
            ui.close();
        }
        if ui.button(crate::i18n::t().copy_as_file).clicked() {
            action = ImgAction::CopyFile;
            ui.close();
        }
    });

    action
}

fn fit_zoom(st: &ImageSession, stage: Rect) -> Option<f32> {
    let px = st.disp_px();
    if px.x < 1.0 || px.y < 1.0 {
        return None;
    }
    let vw = (stage.width() - 16.0).max(0.0);
    let vh = (stage.height() - 16.0).max(0.0);
    if vw < 40.0 || vh < 40.0 {
        return None;
    }
    Some((vw / px.x).min(vh / px.y).clamp(MIN_ZOOM, MAX_ZOOM))
}

fn fit_to_view(st: &mut ImageSession, stage: Rect) -> bool {
    let Some(z) = fit_zoom(st, stage) else {
        return false;
    };
    st.zoom = z;
    st.pan = Vec2::ZERO;
    st.fitted = true;
    true
}

fn zoom_at(st: &mut ImageSession, z: f32, pointer: Option<egui::Pos2>, stage: Rect) {
    let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
    if (z - st.zoom).abs() < 1e-6 {
        return;
    }
    if let Some(p) = pointer {
        let origin = stage.center() + st.pan;
        let ratio = z / st.zoom;
        st.pan = (origin - stage.center()) + (p - origin) * (1.0 - ratio);
    }
    st.zoom = z;
}

fn rotate(st: &mut ImageSession, delta: i32) {
    let q = (st.rot_quarter as i32 + delta).rem_euclid(4);
    st.rot_quarter = q as u8;
    st.fitted = false;
    st.pan = Vec2::ZERO;
}

fn paint_rot(ui: &mut Ui, tex: egui::TextureId, dest: Rect, rot: u8, clip: Rect) {
    let uv = match rot % 4 {
        1 => [pos2(0.0, 1.0), pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0)],
        2 => [pos2(1.0, 1.0), pos2(0.0, 1.0), pos2(0.0, 0.0), pos2(1.0, 0.0)],
        3 => [pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0), pos2(0.0, 0.0)],
        _ => [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)],
    };
    let color = Color32::WHITE;
    let corners = [
        dest.left_top(),
        dest.right_top(),
        dest.right_bottom(),
        dest.left_bottom(),
    ];
    let mut mesh = Mesh::with_texture(tex);
    for i in 0..4 {
        mesh.vertices.push(Vertex {
            pos: corners[i],
            uv: uv[i],
            color,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().with_clip_rect(clip).add(Shape::mesh(mesh));
}

fn load_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| crate::i18n::read_fail(e))?;
    if bytes.is_empty() {
        return Err(crate::i18n::t().file_empty.into());
    }
    let img = image::load_from_memory(&bytes)
        .map_err(|e| crate::i18n::decode_fail(e))?
        .to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}
