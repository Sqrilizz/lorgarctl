use crate::mouse_model::{self, Mesh, View};
use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone)]
pub struct ModelView {
    pub widget: gtk::Box,
    area: gtk::DrawingArea,
    view: Rc<Cell<View>>,
    accent: Rc<Cell<[f32; 3]>>,
}

impl ModelView {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let builder = gtk::Builder::from_string(
            r#"<interface><object class="GtkDrawingArea" id="model"><property name="content-width">280</property><property name="content-height">290</property><property name="hexpand">true</property><property name="vexpand">true</property><property name="focusable">true</property><accessibility><property name="label">Interactive approximate MSA10W model. Arrow keys rotate, plus and minus zoom, Home resets.</property></accessibility></object></interface>"#,
        );
        let area: gtk::DrawingArea = builder.object("model").unwrap_or_default();
        area.add_css_class("mouse-model");
        area.set_cursor_from_name(Some("grab"));
        area.set_tooltip_text(Some("Drag to rotate. Arrow keys rotate, + / − zoom, Home resets. Scroll zooms only while focused."));
        let view = Rc::new(Cell::new(View::default()));
        let accent = Rc::new(Cell::new([0.66, 0.47, 1.0]));
        let mesh = Mesh::msa10w();
        let cached = RefCell::new(None::<(i32, i32, View, [f32; 3], cairo::ImageSurface)>);
        let draw_view = view.clone();
        let draw_accent = accent.clone();
        area.set_draw_func(move |_, cr, width, height| {
            if width < 1 || height < 1 {
                return;
            }
            let sampling = 2.0_f64
                .min(1200.0 / width as f64)
                .min(1000.0 / height as f64);
            let render_width = (width as f64 * sampling).round().clamp(1.0, 1200.0) as i32;
            let render_height = (height as f64 * sampling).round().clamp(1.0, 1000.0) as i32;
            let view = draw_view.get();
            let accent = draw_accent.get();
            let mut cached = cached.borrow_mut();
            if cached.as_ref().is_none_or(|(w, h, v, a, _)| {
                (*w, *h, *v, *a) != (render_width, render_height, view, accent)
            }) {
                let pixels = mouse_model::render(
                    &mesh,
                    view,
                    accent,
                    render_width as usize,
                    render_height as usize,
                );
                match cairo::ImageSurface::create_for_data(
                    pixels,
                    cairo::Format::ARgb32,
                    render_width,
                    render_height,
                    render_width * 4,
                ) {
                    Ok(surface) => {
                        *cached = Some((render_width, render_height, view, accent, surface))
                    }
                    Err(_) => return,
                }
            }
            if cr.save().is_err() {
                return;
            }
            cr.translate(width as f64 * 0.5, height as f64 * 0.81);
            cr.scale(width as f64 * 0.39, height as f64 * 0.075);
            let shadow = cairo::RadialGradient::new(0.0, 0.0, 0.1, 0.0, 0.0, 1.0);
            shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.42);
            shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            let _ = cr.set_source(&shadow);
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            let _ = cr.restore();
            if let Some((w, h, _, _, surface)) = cached.as_ref() {
                if cr.save().is_err() {
                    return;
                }
                cr.scale(width as f64 / *w as f64, height as f64 / *h as f64);
                if cr.set_source_surface(surface, 0.0, 0.0).is_ok() {
                    let _ = cr.paint();
                }
                let _ = cr.restore();
            }
        });
        widget.append(&area);
        let hint = gtk::Label::new(Some("Drag to rotate · Scroll to zoom"));
        hint.add_css_class("small");
        widget.append(&hint);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.set_halign(gtk::Align::Center);
        let result = Self {
            widget,
            area,
            view,
            accent,
        };
        for (text, tooltip, action) in [
            ("Zoom −", "Zoom out", 0),
            ("Zoom +", "Zoom in", 1),
            ("Top", "View from above", 2),
            ("Reset", "Reset model view", 3),
        ] {
            let button = gtk::Button::with_label(text);
            button.add_css_class("secondary");
            button.set_tooltip_text(Some(tooltip));
            let view = result.view.clone();
            let area = result.area.downgrade();
            button.connect_clicked(move |_| {
                let mut camera = view.get();
                match action {
                    0 => camera.zoom_by(0.88),
                    1 => camera.zoom_by(1.14),
                    2 => {
                        camera = View {
                            yaw: 0.0,
                            pitch: std::f32::consts::FRAC_PI_2,
                            zoom: 1.0,
                        }
                    }
                    _ => camera = View::default(),
                }
                view.set(camera);
                if let Some(area) = area.upgrade() {
                    area.queue_draw();
                }
            });
            controls.append(&button);
        }
        result.widget.append(&controls);
        let disclosure = gtk::Label::new(Some("Visual approximation · not a hardware readback"));
        disclosure.add_css_class("small");
        disclosure.set_wrap(true);
        result.widget.append(&disclosure);
        result.bind();
        result
    }

    pub fn set_accent(&self, color: Option<(u8, u8, u8)>) {
        let next = color
            .map(|(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
            .unwrap_or([0.66, 0.47, 1.0]);
        if self.accent.replace(next) != next {
            self.area.queue_draw();
        }
    }

    fn bind(&self) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let start = Rc::new(Cell::new(View::default()));
        let view = self.view.clone();
        let origin = start.clone();
        let area = self.area.downgrade();
        drag.connect_drag_begin(move |_, _, _| {
            origin.set(view.get());
            if let Some(area) = area.upgrade() {
                area.grab_focus();
                area.set_cursor_from_name(Some("grabbing"));
            }
        });
        let view = self.view.clone();
        let area = self.area.downgrade();
        drag.connect_drag_update(move |_, x, y| {
            let mut camera = start.get();
            camera.orbit(x as f32 * 0.012, y as f32 * 0.012);
            view.set(camera);
            if let Some(area) = area.upgrade() {
                area.queue_draw();
            }
        });
        let area = self.area.downgrade();
        drag.connect_drag_end(move |_, _, _| {
            if let Some(area) = area.upgrade() {
                area.set_cursor_from_name(Some("grab"));
            }
        });
        self.area.add_controller(drag);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let view = self.view.clone();
        let area = self.area.downgrade();
        scroll.connect_scroll(move |_, _, dy| {
            let Some(area) = area.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !area.has_focus() {
                return glib::Propagation::Proceed;
            }
            let mut camera = view.get();
            camera.zoom_by((-dy as f32 * 0.12).exp());
            view.set(camera);
            area.queue_draw();
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);
        let keys = gtk::EventControllerKey::new();
        let view = self.view.clone();
        let area = self.area.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK,
            ) {
                return glib::Propagation::Proceed;
            }
            let mut camera = view.get();
            match key {
                gdk::Key::Left => camera.orbit(-0.15, 0.0),
                gdk::Key::Right => camera.orbit(0.15, 0.0),
                gdk::Key::Up => camera.orbit(0.0, -0.15),
                gdk::Key::Down => camera.orbit(0.0, 0.15),
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => camera.zoom_by(1.14),
                gdk::Key::minus | gdk::Key::KP_Subtract => camera.zoom_by(0.88),
                gdk::Key::Home => camera = View::default(),
                _ => return glib::Propagation::Proceed,
            }
            view.set(camera);
            if let Some(area) = area.upgrade() {
                area.queue_draw();
            }
            glib::Propagation::Stop
        });
        self.area.add_controller(keys);
    }
}
