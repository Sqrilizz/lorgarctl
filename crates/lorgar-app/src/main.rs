mod backend;
mod model_view;
mod mouse_model;

use backend::{Update, Work};
use clap::Parser;
use gtk::glib;
use gtk::prelude::*;
use lorgar_core::profiles::{AppRule, Applications, Config, Profile};
use lorgar_core::LightingMode;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Parser)]
#[command(version, about = "Native Linux settings for the LORGAR MSA10W")]
pub struct Options {
    #[arg(long, help = "Override the profile configuration file")]
    config: Option<PathBuf>,
    #[arg(long, help = "Connect to an existing daemon at this socket")]
    socket: Option<PathBuf>,
    #[arg(long, help = "Do not start a temporary lorgard when none is running")]
    no_start_daemon: bool,
    #[arg(
        long,
        help = "Show the unfinished experimental 3D preview instead of the product photo"
    )]
    experimental_3d: bool,
    #[arg(
        long,
        help = "Export the approximate 3D mesh as OBJ/MTL to a new directory, without opening HID or the app"
    )]
    export_model: Option<PathBuf>,
}

#[derive(Default)]
struct Model {
    config: Config,
    selected: Option<String>,
    baseline: Profile,
    dirty: bool,
    quiet: bool,
    busy: bool,
    connected: bool,
    config_valid: bool,
    discard_on_close: bool,
}

#[allow(dead_code)]
struct Ui {
    window: gtk::ApplicationWindow,
    model: RefCell<Model>,
    tx: Sender<Work>,
    stack: gtk::Stack,
    editor: gtk::Box,
    profile_list: gtk::Box,
    profile_name: gtk::Entry,
    draft_label: gtk::Label,
    feedback: gtk::Label,
    connection: gtk::Label,
    connection_detail: gtk::Label,
    battery: gtk::Label,
    active_stage: gtk::Label,
    applied: gtk::Label,
    dpi: Vec<(gtk::CheckButton, gtk::SpinButton)>,
    polling: gtk::DropDown,
    stage: gtk::DropDown,
    lighting: gtk::DropDown,
    use_color: gtk::CheckButton,
    color: gtk::Entry,
    color_css: gtk::CssProvider,
    save: gtk::Button,
    apply: gtk::Button,
    reset: gtk::Button,
    create: gtk::Button,
    delete: gtk::Button,
    restore: gtk::CheckButton,
    default_profile: gtk::DropDown,
    rule_process: gtk::Entry,
    rule_profile: gtk::DropDown,
    rules: gtk::Box,
    add_rule: gtk::Button,
    save_preferences: gtk::Button,
    app_draft: RefCell<Applications>,
    nav: Vec<gtk::Button>,
    mouse_model: Option<model_view::ModelView>,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class(class);
    label
}

fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}
fn button(text: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class(class);
    button
}
fn section(title: &str, subtitle: &str) -> gtk::Box {
    let box_ = column(6);
    box_.append(&label(title, "title"));
    let description = label(subtitle, "muted");
    description.set_wrap(true);
    box_.append(&description);
    box_
}
fn card() -> gtk::Box {
    let box_ = column(16);
    box_.add_css_class("card");
    box_
}
fn field(caption: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let box_ = column(8);
    let caption = label(caption, "field-label");
    caption.set_mnemonic_widget(Some(widget));
    box_.append(&caption);
    box_.append(widget);
    box_
}
fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build()
}

impl Ui {
    fn new(app: &gtk::Application, tx: Sender<Work>, experimental_3d: bool) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Lorgar Control")
            .default_width(1180)
            .default_height(820)
            .build();
        window.set_size_request(880, 640);
        let header = gtk::HeaderBar::new();
        header.set_title_widget(Some(&label("Lorgar Control", "window-title")));

        // App logo in titlebar
        let icon_bytes = glib::Bytes::from_static(include_bytes!("../assets/app_icon.png"));
        if let Ok(texture) = gtk::gdk::Texture::from_bytes(&icon_bytes) {
            let icon_pic = gtk::Picture::for_paintable(&texture);
            icon_pic.set_height_request(22);
            icon_pic.set_width_request(22);
            header.pack_start(&icon_pic);
        }

        window.set_titlebar(Some(&header));
        let root = row(0);
        let sidebar = column(12);
        sidebar.set_width_request(210);
        sidebar.add_css_class("sidebar");
        let wordmark = section("LORGAR", "MSA10W GAMING MOUSE");
        wordmark.add_css_class("wordmark");
        sidebar.append(&wordmark);
        sidebar.append(&label("CONNECTED DEVICE", "eyebrow"));
        let device_name = label("LORGAR MSA10W", "device-name");
        sidebar.append(&device_name);
        let connection = label("Connecting…", "connection");
        connection.add_css_class("status-pill");
        connection.add_css_class("pill-online");
        connection.set_wrap(true);
        sidebar.append(&connection);
        let mut nav = Vec::new();
        for name in [
            "Overview",
            "Sensitivity",
            "Lighting",
            "Profiles",
            "Automatic profiles",
        ] {
            let item = button(name, "nav");
            sidebar.append(&item);
            nav.push(item);
        }
        let spacer = column(0);
        spacer.set_vexpand(true);
        sidebar.append(&spacer);
        sidebar.append(&label("PixArt PAW3395 · 1000 Hz", "small"));
        sidebar.append(&label("Native Linux Stack · v0.1", "small"));
        root.append(&sidebar);
        let body = column(0);
        body.set_hexpand(true);
        let top = row(16);
        top.add_css_class("toolbar");
        let draft_label = label("Unsaved draft", "field-label");
        draft_label.set_hexpand(true);
        top.append(&draft_label);
        let reconnect = button("Reconnect", "secondary");
        top.append(&reconnect);
        body.append(&top);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);

        let overview = column(24);
        overview.add_css_class("page");
        overview.append(&section(
            "Device Overview",
            "Hardware status, battery level, active DPI stage and profile for your LORGAR MSA10W.",
        ));
        let hero = row(24);
        hero.set_homogeneous(true);
        let showcase = card();
        showcase.add_css_class("showcase");

        let mouse_model = experimental_3d.then(model_view::ModelView::new);
        if let Some(model) = &mouse_model {
            showcase.append(&label("3D INTERACTIVE VIEW", "eyebrow"));
            showcase.append(&model.widget);
        } else {
            let header_row = row(8);
            let title_box = column(2);
            title_box.set_hexpand(true);
            title_box.append(&label("LORGAR MSA10W WIRELESS", "eyebrow"));
            title_box.append(&label("PixArt PAW3395 · 26,000 DPI · 1000 Hz", "small"));
            header_row.append(&title_box);
            let view_toggle = button("View: Top", "secondary");
            header_row.append(&view_toggle);
            showcase.append(&header_row);

            let top_bytes = glib::Bytes::from_static(include_bytes!("../assets/mouse_top.png"));
            let side_bytes = glib::Bytes::from_static(include_bytes!("../assets/mouse_side.png"));
            let top_texture = gtk::gdk::Texture::from_bytes(&top_bytes).ok();
            let side_texture = gtk::gdk::Texture::from_bytes(&side_bytes).ok();

            let picture = gtk::Picture::new();
            if let Some(tex) = &top_texture {
                picture.set_paintable(Some(tex));
            }
            picture.set_content_fit(gtk::ContentFit::Contain);
            picture.set_can_shrink(true);
            picture.set_height_request(260);
            showcase.append(&picture);

            let is_top = Rc::new(RefCell::new(true));
            let top_tex_clone = top_texture;
            let side_tex_clone = side_texture;
            let pic_clone = picture;
            let toggle_btn = view_toggle;
            toggle_btn.clone().connect_clicked(move |_| {
                let mut top = is_top.borrow_mut();
                *top = !*top;
                if *top {
                    toggle_btn.set_label("View: Top");
                    if let Some(tex) = &top_tex_clone {
                        pic_clone.set_paintable(Some(tex));
                    }
                } else {
                    toggle_btn.set_label("View: Side");
                    if let Some(tex) = &side_tex_clone {
                        pic_clone.set_paintable(Some(tex));
                    }
                }
            });
        }
        hero.append(&showcase);

        let stats = column(16);
        let battery_card = card();
        battery_card.append(&label("BATTERY LEVEL", "eyebrow"));
        let battery = label("—", "metric");
        battery_card.append(&battery);
        battery_card.append(&label("Wireless power supply status", "small"));
        stats.append(&battery_card);

        let stage_card = card();
        stage_card.append(&label("ACTIVE DPI STAGE", "eyebrow"));
        let active_stage = label("Unknown", "stat-value");
        stage_card.append(&active_stage);
        stage_card.append(&label("Onboard PixArt sensor stage", "small"));
        stats.append(&stage_card);

        let applied_card = card();
        applied_card.append(&label("ACTIVE PROFILE", "eyebrow"));
        let applied = label("None", "stat-value");
        applied.set_wrap(true);
        applied_card.append(&applied);
        applied_card.append(&label("Configured hardware profile", "small"));
        stats.append(&applied_card);
        hero.append(&stats);
        overview.append(&hero);

        let connection_detail = label("Connecting to the device service…", "muted");
        connection_detail.set_wrap(true);
        overview.append(&connection_detail);

        let action_bar = row(12);
        let customize = button("Customize Sensitivity", "primary");
        action_bar.append(&customize);
        overview.append(&action_bar);
        stack.add_named(&scroll(&overview), Some("overview"));

        let sensitivity = column(24);
        sensitivity.add_css_class("page");
        sensitivity.append(&section(
            "Sensor & Sensitivity",
            "Configure onboard DPI stages, polling rate, and quick sensitivity presets.",
        ));
        sensitivity.append(&label("SENSITIVITY PRESETS", "eyebrow"));
        let presets = row(10);
        let preset_buttons = [
            ("FPS · 400–3200", [400, 800, 1600, 3200]),
            ("Balanced · 800–6400", [800, 1600, 3200, 6400]),
            ("High · 1600–12000", [1600, 3200, 6400, 12000]),
        ];
        let mut preset_actions = Vec::new();
        for (title, values) in preset_buttons {
            let preset = button(title, "secondary");
            preset.set_hexpand(true);
            presets.append(&preset);
            preset_actions.push((preset, values));
        }
        sensitivity.append(&presets);
        let preset_note = label(
            "Presets populate stages 0–3 in the draft and select stage 0. Use 'Apply to mouse' to commit.",
            "small",
        );
        preset_note.set_wrap(true);
        sensitivity.append(&preset_note);

        sensitivity.append(&label("ONBOARD DPI STAGES (1–8)", "eyebrow"));
        let dpi_grid = gtk::Grid::builder()
            .row_spacing(12)
            .column_spacing(12)
            .column_homogeneous(true)
            .build();
        let mut dpi = Vec::new();
        let stage_color_classes = [
            "color-stage0",
            "color-stage1",
            "color-stage2",
            "color-stage3",
            "color-stage4",
            "color-stage5",
            "color-stage6",
            "color-stage7",
        ];
        for (index, value) in [800., 1600., 2400., 3200., 4800., 6400., 8000., 12000.]
            .iter()
            .enumerate()
        {
            let box_ = card();
            box_.add_css_class("dpi-card");
            let header_row = row(8);
            let dot = label("●", "swatch-dot");
            dot.add_css_class(stage_color_classes[index]);
            header_row.append(&dot);
            let include = gtk::CheckButton::with_label(&format!("Stage {index}"));
            include.set_tooltip_text(Some("Include this stage when saving or applying"));
            header_row.append(&include);
            box_.append(&header_row);

            let spin = gtk::SpinButton::with_range(100., 26000., 100.);
            spin.set_value(*value);
            spin.set_numeric(true);
            spin.set_sensitive(false);
            spin.set_width_chars(6);
            spin.set_tooltip_text(Some(&format!("DPI for stage {index}, 100 to 26,000")));
            box_.append(&spin);
            box_.append(&label("DPI", "small"));
            dpi_grid.attach(&box_, (index % 4) as i32, (index / 4) as i32, 1, 1);
            dpi.push((include, spin));
        }
        sensitivity.append(&dpi_grid);

        let settings = row(24);
        settings.set_homogeneous(true);
        let stage = gtk::DropDown::from_strings(&[
            "Leave unchanged",
            "Stage 0",
            "Stage 1",
            "Stage 2",
            "Stage 3",
            "Stage 4",
            "Stage 5",
            "Stage 6",
            "Stage 7",
        ]);
        let polling = gtk::DropDown::from_strings(&[
            "Leave unchanged",
            "125 Hz · 8 ms",
            "250 Hz · 4 ms",
            "500 Hz · 2 ms",
            "1000 Hz · 1 ms",
        ]);
        settings.append(&field("Active Stage", &stage));
        settings.append(&field("Report / Polling Rate", &polling));
        sensitivity.append(&settings);

        // Hardware details card reversed from official app
        let hw_card = card();
        hw_card.append(&label(
            "HARDWARE SPECIFICATIONS & ONBOARD CONTROLS",
            "eyebrow",
        ));
        let hw_specs = row(16);
        hw_specs.set_homogeneous(true);

        let spec1 = column(4);
        spec1.add_css_class("spec-box");
        spec1.append(&label("OPTICAL SENSOR", "eyebrow"));
        spec1.append(&label(
            "PixArt PAW3395 (Up to 26,000 DPI, 650 IPS, 50G)",
            "small",
        ));
        hw_specs.append(&spec1);

        let spec2 = column(4);
        spec2.add_css_class("spec-box");
        spec2.append(&label("LIFT-OFF DISTANCE", "eyebrow"));
        spec2.append(&label("1.0 mm / 2.0 mm Ultra-low Tracking Cutoff", "small"));
        hw_specs.append(&spec2);

        let spec3 = column(4);
        spec3.add_css_class("spec-box");
        spec3.append(&label("PHYSICAL BUTTONS", "eyebrow"));
        spec3.append(&label(
            "6 Programmable Switches (L, R, Wheel, Side1, Side2, DPI)",
            "small",
        ));
        hw_specs.append(&spec3);

        hw_card.append(&hw_specs);
        sensitivity.append(&hw_card);

        stack.add_named(&scroll(&sensitivity), Some("sensitivity"));

        let lighting_page = column(24);
        lighting_page.add_css_class("page");
        lighting_page.append(&section(
            "RGB Lighting",
            "Select lighting animations, presets, and customized RGB colors.",
        ));
        let lighting_card = card();
        let lighting = gtk::DropDown::from_strings(&[
            "Leave unchanged",
            "Off",
            "Static",
            "Breathing",
            "Color cycle",
            "Tapping",
            "Wave",
        ]);
        lighting_card.append(&field("Lighting Animation", &lighting));
        let use_color = gtk::CheckButton::with_label("Enable custom color");
        lighting_card.append(&use_color);
        let color = gtk::Entry::builder()
            .text("6366f1")
            .max_length(7)
            .placeholder_text("RRGGBB")
            .build();
        color.set_sensitive(false);
        let color_row = row(16);
        let preview = column(0);
        preview.add_css_class("color-preview");
        preview.set_size_request(52, 52);
        color_row.append(&preview);
        let color_field = field("Hex Color Code", &color);
        color_field.set_hexpand(true);
        color_row.append(&color_field);
        lighting_card.append(&color_row);
        let swatches = row(10);
        let mut swatch_buttons = Vec::new();
        for (name, hex) in [
            ("Violet", "9900ff"),
            ("Red", "dc143c"),
            ("Amber", "ff7f27"),
            ("Green", "32cd32"),
            ("Blue", "1e90ff"),
            ("White", "ffffff"),
        ] {
            let swatch = button(name, "swatch");
            swatch.add_css_class(&name.to_lowercase());
            swatch.set_hexpand(true);
            swatches.append(&swatch);
            swatch_buttons.push((swatch, hex));
        }
        lighting_card.append(&swatches);
        lighting_page.append(&lighting_card);
        let note = label("Colors are transmitted via standard HID feature packets directly to the MSA10W microcontroller.", "muted");
        note.set_wrap(true);
        lighting_page.append(&note);
        stack.add_named(&scroll(&lighting_page), Some("lighting"));

        let profiles_page = column(24);
        profiles_page.add_css_class("page");
        profiles_page.append(&section(
            "Profile Management",
            "Save sensitivity, polling, and lighting setups into named profiles.",
        ));
        let new_row = row(12);
        let profile_name = gtk::Entry::builder()
            .placeholder_text("gaming")
            .hexpand(true)
            .max_length(64)
            .build();
        new_row.append(&field("New profile name", &profile_name));
        let create = button("Create profile", "secondary");
        create.set_valign(gtk::Align::End);
        new_row.append(&create);
        profiles_page.append(&new_row);
        let profile_list = column(10);
        profiles_page.append(&profile_list);
        let delete = button("Delete selected profile", "danger");
        delete.set_halign(gtk::Align::Start);
        profiles_page.append(&delete);
        stack.add_named(&scroll(&profiles_page), Some("profiles"));

        let auto_page = column(24);
        auto_page.add_css_class("page");
        auto_page.append(&section(
            "Automatic Switching & Preferences",
            "Automatically apply profiles when applications launch or when the mouse reconnects.",
        ));
        let restore = gtk::CheckButton::with_label(
            "Restore the last applied profile when the mouse reconnects",
        );
        auto_page.append(&restore);
        let default_profile = gtk::DropDown::from_strings(&["No default"]);
        auto_page.append(&field("Default application profile", &default_profile));

        let native_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let availability = label(
            if native_wayland {
                "Active desktop: Wayland session. The background daemon automatically tracks active processes using native KWin DBus or X11 EWMH."
            } else {
                "Active desktop: X11 session. Process tracking utilizes standard EWMH window inspection."
            },
            "notice",
        );
        availability.set_wrap(true);
        auto_page.append(&availability);
        let rule_row = row(12);
        let rule_process = gtk::Entry::builder()
            .placeholder_text("cs2")
            .hexpand(true)
            .build();
        let rule_profile = gtk::DropDown::from_strings(&["Choose profile"]);
        rule_row.append(&field("Executable name", &rule_process));
        rule_row.append(&field("Profile", &rule_profile));
        let add_rule = button("Add rule", "secondary");
        add_rule.set_valign(gtk::Align::End);
        rule_row.append(&add_rule);
        auto_page.append(&rule_row);
        let rules = column(8);
        auto_page.append(&rules);
        let save_preferences = button("Save automatic preferences", "secondary");
        save_preferences.set_halign(gtk::Align::Start);
        auto_page.append(&save_preferences);
        stack.add_named(&scroll(&auto_page), Some("automatic"));

        let editor = column(0);
        editor.set_vexpand(true);
        editor.append(&stack);
        body.append(&editor);
        let footer = row(12);
        footer.add_css_class("footer");
        let feedback = label(
            "Select a profile or configure draft settings to apply to the mouse.",
            "feedback",
        );
        feedback.set_wrap(true);
        feedback.set_hexpand(true);
        feedback.set_max_width_chars(50);
        footer.append(&feedback);
        let reset = button("Reset draft", "secondary");
        footer.append(&reset);
        let save = button("Save profile", "secondary");
        footer.append(&save);
        let apply = button("Apply to mouse", "primary");
        footer.append(&apply);
        body.append(&footer);
        root.append(&body);
        window.set_child(Some(&root));
        let color_css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&window),
            &color_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let ui = Rc::new(Self {
            window,
            model: RefCell::new(Model::default()),
            tx,
            stack,
            editor,
            profile_list,
            profile_name,
            draft_label,
            feedback,
            connection,
            connection_detail,
            battery,
            active_stage,
            applied,
            dpi,
            polling,
            stage,
            lighting,
            use_color,
            color,
            color_css,
            save,
            apply,
            reset,
            create,
            delete,
            restore,
            default_profile,
            rule_process,
            rule_profile,
            rules,
            add_rule,
            save_preferences,
            app_draft: RefCell::new(Applications::default()),
            nav,
            mouse_model,
        });
        ui.bind();
        Self::clicked(&ui, &reconnect, |ui| {
            let _ = ui.tx.send(Work::Connect);
            ui.connection.set_text("Connecting…");
        });
        Self::clicked(&ui, &customize, |ui| ui.navigate(1));
        for (preset, values) in preset_actions {
            Self::clicked(&ui, &preset, move |ui| ui.set_dpi_preset(values));
        }
        for (swatch, hex) in swatch_buttons {
            Self::clicked(&ui, &swatch, move |ui| {
                ui.use_color.set_active(true);
                ui.color.set_text(hex);
            });
        }
        ui.navigate(0);
        ui.refresh_actions();
        ui
    }

    fn clicked(ui: &Rc<Self>, button: &gtk::Button, action: impl Fn(&Rc<Self>) + 'static) {
        let weak = Rc::downgrade(ui);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                action(&ui);
            }
        });
    }

    fn navigate(&self, index: usize) {
        self.stack.set_visible_child_name(
            [
                "overview",
                "sensitivity",
                "lighting",
                "profiles",
                "automatic",
            ][index],
        );
        for (i, nav) in self.nav.iter().enumerate() {
            if i == index {
                nav.add_css_class("active");
            } else {
                nav.remove_css_class("active");
            }
        }
    }

    fn set_dpi_preset(&self, values: [u16; 4]) {
        for (index, (included, spin)) in self.dpi.iter().enumerate() {
            let value = values.get(index).copied();
            included.set_active(value.is_some());
            spin.set_sensitive(value.is_some());
            if let Some(value) = value {
                spin.set_value(value as f64);
            }
        }
        self.stage.set_selected(1);
        self.changed();
        self.message(
            "Preset added to the draft. Apply to mouse when ready.",
            false,
        );
    }

    fn message(&self, text: &str, error: bool) {
        self.feedback.set_text(text);
        if error {
            self.feedback.add_css_class("error");
        } else {
            self.feedback.remove_css_class("error");
        }
    }

    fn draft(&self) -> lorgar_core::Result<Profile> {
        let mut profile = self.model.borrow().baseline.clone();
        profile.dpi_stages.clear();
        for (index, (included, spin)) in self.dpi.iter().enumerate() {
            if included.is_active() {
                profile
                    .dpi_stages
                    .insert(index as u8, spin.value_as_int() as u16);
            }
        }
        profile.active_stage = self.stage.selected().checked_sub(1).map(|i| i as u8);
        profile.polling = [None, Some(125), Some(250), Some(500), Some(1000)]
            .get(self.polling.selected() as usize)
            .copied()
            .flatten();
        profile.rgb = [
            None,
            Some(LightingMode::Off),
            Some(LightingMode::Static),
            Some(LightingMode::Breathing),
            Some(LightingMode::ColorCycle),
            Some(LightingMode::Tapping),
            Some(LightingMode::Wave),
        ]
        .get(self.lighting.selected() as usize)
        .copied()
        .flatten();
        profile.color = self
            .use_color
            .is_active()
            .then(|| self.color.text().trim().trim_start_matches('#').to_string());
        profile.commands()?;
        Ok(profile)
    }

    fn preferences(&self) -> (bool, Applications) {
        let mut applications = self.app_draft.borrow().clone();
        applications.default = self
            .default_profile
            .selected()
            .checked_sub(1)
            .and_then(|i| {
                self.model
                    .borrow()
                    .config
                    .profiles
                    .keys()
                    .nth(i as usize)
                    .cloned()
            });
        (self.restore.is_active(), applications)
    }

    fn preferences_dirty(&self) -> bool {
        let current = self.preferences();
        let model = self.model.borrow();
        current.0 != model.config.restore_on_reconnect || current.1 != model.config.applications
    }

    fn changed(&self) {
        if self.model.borrow().quiet {
            return;
        }
        let dirty = match self.draft() {
            Ok(draft) => draft != self.model.borrow().baseline,
            Err(_) => true,
        };
        self.model.borrow_mut().dirty = dirty;
        self.refresh_actions();
        self.update_color_preview();
    }

    fn update_color_preview(&self) {
        let hex = self.color.text().trim().trim_start_matches('#').to_string();
        if hex.len() == 6 && u32::from_str_radix(&hex, 16).is_ok() {
            self.color.remove_css_class("invalid");
            self.color_css
                .load_from_data(&format!(".color-preview {{ background: #{hex}; }}"));
        } else if self.use_color.is_active() {
            self.color.add_css_class("invalid");
        }
    }

    fn refresh_actions(&self) {
        let model = self.model.borrow();
        let selected = model.selected.is_some();
        let dirty = model.dirty;
        let busy = model.busy;
        let connected = model.connected;
        let valid = model.config_valid;
        self.apply
            .set_sensitive(!busy && valid && (connected || dirty));
        self.save
            .set_sensitive(!busy && valid && (dirty || !selected));
        self.reset.set_sensitive(!busy && dirty && selected);
        self.create.set_sensitive(!busy && valid);
        self.delete.set_sensitive(!busy && valid && selected);
        self.draft_label.set_visible(dirty);
    }

    fn send(&self, work: Work) {
        self.model.borrow_mut().busy = true;
        self.refresh_actions();
        let _ = self.tx.send(work);
    }

    fn load_profile(&self, name: Option<String>) {
        let mut model = self.model.borrow_mut();
        model.selected = name.clone();
        let profile = name
            .as_ref()
            .and_then(|n| model.config.profiles.get(n))
            .cloned()
            .unwrap_or_default();
        model.baseline = profile.clone();
        model.dirty = false;
        model.quiet = true;
        drop(model);

        for (index, (included, spin)) in self.dpi.iter().enumerate() {
            let value = profile.dpi_stages.get(&(index as u8));
            included.set_active(value.is_some());
            spin.set_sensitive(value.is_some());
            if let Some(value) = value {
                spin.set_value(*value as f64);
            }
        }
        self.stage
            .set_selected(profile.active_stage.map(|s| s as u32 + 1).unwrap_or(0));
        self.polling.set_selected(match profile.polling {
            Some(125) => 1,
            Some(250) => 2,
            Some(500) => 3,
            Some(1000) => 4,
            _ => 0,
        });
        self.lighting.set_selected(match profile.rgb {
            Some(LightingMode::Off) => 1,
            Some(LightingMode::Static) => 2,
            Some(LightingMode::Breathing) => 3,
            Some(LightingMode::ColorCycle) => 4,
            Some(LightingMode::Tapping) => 5,
            Some(LightingMode::Wave) => 6,
            None => 0,
        });
        self.use_color.set_active(profile.color.is_some());
        self.color.set_sensitive(profile.color.is_some());
        self.color
            .set_text(profile.color.as_deref().unwrap_or("6366f1"));
        self.model.borrow_mut().quiet = false;
        self.changed();
    }

    fn bind(self: &Rc<Self>) {
        for (index, nav) in self.nav.iter().enumerate() {
            Self::clicked(self, nav, move |ui| ui.navigate(index));
        }
        for (included, spin) in &self.dpi {
            let weak = Rc::downgrade(self);
            let spin_clone = spin.clone();
            included.connect_toggled(move |check| {
                spin_clone.set_sensitive(check.is_active());
                if let Some(ui) = weak.upgrade() {
                    ui.changed();
                }
            });
            let weak = Rc::downgrade(self);
            spin.connect_value_changed(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.changed();
                }
            });
        }
        for dropdown in [&self.polling, &self.stage, &self.lighting] {
            let weak = Rc::downgrade(self);
            dropdown.connect_selected_notify(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.changed();
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.color.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.changed();
            }
        });
        let weak = Rc::downgrade(self);
        self.use_color.connect_toggled(move |check| {
            if let Some(ui) = weak.upgrade() {
                ui.color.set_sensitive(check.is_active());
                ui.changed();
            }
        });
        Self::clicked(self, &self.reset, |ui| {
            let selected = ui.model.borrow().selected.clone();
            ui.load_profile(selected);
            ui.message("Draft reset to the saved profile.", false);
        });
        Self::clicked(self, &self.save, |ui| {
            let model = ui.model.borrow();
            let Some(name) = model.selected.clone() else {
                drop(model);
                ui.navigate(3);
                ui.message(
                    "Name your new profile and choose Create to save this draft.",
                    false,
                );
                return;
            };
            let expected = Some(model.baseline.clone());
            drop(model);
            match ui.draft() {
                Ok(profile) => ui.send(Work::Save {
                    name,
                    profile,
                    expected,
                }),
                Err(e) => ui.message(&e.to_string(), true),
            }
        });
        Self::clicked(self, &self.apply, |ui| match ui.draft() {
            Ok(profile) => {
                let model = ui.model.borrow();
                let name = if model.dirty {
                    None
                } else {
                    model.selected.clone()
                };
                drop(model);
                ui.send(Work::Apply { name, profile });
            }
            Err(e) => ui.message(&e.to_string(), true),
        });
        Self::clicked(self, &self.create, |ui| {
            if ui.model.borrow().dirty && ui.model.borrow().selected.is_some() {
                ui.message(
                    "Save or reset your draft before creating another profile.",
                    true,
                );
                return;
            }
            let profile = if ui.model.borrow().selected.is_none() {
                match ui.draft() {
                    Ok(profile) => profile,
                    Err(error) => {
                        ui.message(&error.to_string(), true);
                        return;
                    }
                }
            } else {
                Profile::default()
            };
            ui.send(Work::Create {
                name: ui.profile_name.text().trim().into(),
                profile,
            });
        });
        Self::clicked(self, &self.delete, |ui| {
            let model = ui.model.borrow();
            let Some(name) = model.selected.clone() else {
                return;
            };
            let expected = model.baseline.clone();
            drop(model);
            let weak = Rc::downgrade(ui);
            confirm(
                &ui.window,
                "Delete profile?",
                &format!("Delete {name}? Your mouse settings will stay as they are."),
                "Delete",
                move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.send(Work::Delete {
                            name: name.clone(),
                            expected: expected.clone(),
                        });
                    }
                },
            );
        });
        Self::clicked(self, &self.add_rule, |ui| {
            let process = ui.rule_process.text().trim().to_string();
            let index = ui.rule_profile.selected().checked_sub(1);
            let profile = index.and_then(|i| {
                ui.model
                    .borrow()
                    .config
                    .profiles
                    .keys()
                    .nth(i as usize)
                    .cloned()
            });
            let Some(profile) = profile else {
                ui.message("Choose a saved profile for this rule.", true);
                return;
            };
            if process.is_empty() {
                ui.message("Enter an executable name, such as cs2.", true);
                return;
            }
            ui.app_draft.borrow_mut().rules.push(AppRule {
                process,
                profile,
                extra: Default::default(),
            });
            ui.rule_process.set_text("");
            ui.render_rules();
        });
        Self::clicked(self, &self.save_preferences, |ui| {
            let mut applications = ui.app_draft.borrow().clone();
            applications.default = ui.default_profile.selected().checked_sub(1).and_then(|i| {
                ui.model
                    .borrow()
                    .config
                    .profiles
                    .keys()
                    .nth(i as usize)
                    .cloned()
            });
            ui.send(Work::Preferences {
                restore: ui.restore.is_active(),
                applications,
                expected: {
                    let model = ui.model.borrow();
                    (
                        model.config.restore_on_reconnect,
                        model.config.applications.clone(),
                    )
                },
            });
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |window| {
            if let Some(ui) = weak.upgrade() {
                if !ui.model.borrow().discard_on_close
                    && (ui.model.borrow().dirty || ui.preferences_dirty())
                {
                    let weak = Rc::downgrade(&ui);
                    confirm(
                        window,
                        "Discard unsaved changes?",
                        "Your profile or automatic-profile preferences have unsaved changes.",
                        "Discard and close",
                        move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.model.borrow_mut().discard_on_close = true;
                                ui.window.close();
                            }
                        },
                    );
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }

    fn render_rules(self: &Rc<Self>) {
        while let Some(child) = self.rules.first_child() {
            self.rules.remove(&child);
        }
        for (index, rule) in self.app_draft.borrow().rules.iter().enumerate() {
            let line = row(12);
            line.add_css_class("profile-row");
            let text = label(
                &format!("{}  →  {}", rule.process, rule.profile),
                "field-label",
            );
            text.set_hexpand(true);
            text.set_wrap(true);
            line.append(&text);
            let remove = button("Remove", "secondary");
            line.append(&remove);
            self.rules.append(&line);
            Self::clicked(self, &remove, move |ui| {
                if index < ui.app_draft.borrow().rules.len() {
                    ui.app_draft.borrow_mut().rules.remove(index);
                }
                ui.render_rules();
            });
        }
    }

    fn render_profiles(self: &Rc<Self>, preferences: Option<(bool, Applications)>) {
        while let Some(child) = self.profile_list.first_child() {
            self.profile_list.remove(&child);
        }
        let profiles = self.model.borrow().config.profiles.clone();
        if profiles.is_empty() {
            self.profile_list.append(&label(
                "No profiles yet. Create one to start saving your setup.",
                "muted",
            ));
        }
        for (name, profile) in &profiles {
            let line = row(16);
            line.add_css_class("profile-row");
            let titles = column(4);
            titles.set_hexpand(true);
            titles.append(&label(name, "card-title"));
            titles.append(&label(
                &format!(
                    "{} DPI stages · {}",
                    profile.dpi_stages.len(),
                    profile
                        .polling
                        .map(|p| format!("{p} Hz"))
                        .unwrap_or_else(|| "polling unchanged".into())
                ),
                "small",
            ));
            line.append(&titles);
            let load = button("Load profile", "secondary");
            line.append(&load);
            self.profile_list.append(&line);
            let name = name.clone();
            Self::clicked(self, &load, move |ui| {
                if ui.model.borrow().dirty {
                    ui.message(
                        "Save or reset your draft before loading another profile.",
                        true,
                    );
                    return;
                }
                ui.load_profile(Some(name.clone()));
                ui.navigate(1);
                ui.message(
                    "Profile loaded into the editor. Apply when you're ready.",
                    false,
                );
            });
        }
        let mut names = vec!["No default".to_string()];
        names.extend(profiles.keys().cloned());
        self.default_profile.set_model(Some(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        names[0] = "Choose profile".into();
        self.rule_profile.set_model(Some(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        let (restore, applications) = preferences.unwrap_or_else(|| {
            let model = self.model.borrow();
            (
                model.config.restore_on_reconnect,
                model.config.applications.clone(),
            )
        });
        self.restore.set_active(restore);
        self.default_profile.set_selected(
            applications
                .default
                .as_ref()
                .and_then(|name| names.iter().position(|n| n == name))
                .unwrap_or(0) as u32,
        );
        *self.app_draft.borrow_mut() = applications;
        self.render_rules();
    }

    fn update(self: &Rc<Self>, update: Update) {
        match update {
            Update::Config(Ok(config)) => {
                let preferences = self.preferences_dirty().then(|| self.preferences());
                let valid = config.validate().is_ok();
                {
                    let mut model = self.model.borrow_mut();
                    model.config = config;
                    model.config_valid = valid;
                }
                self.render_profiles(preferences);
                let selected = {
                    let model = self.model.borrow();
                    if model.selected.is_none() && !model.dirty {
                        model
                            .config
                            .last_profile
                            .clone()
                            .or_else(|| model.config.profiles.keys().next().cloned())
                    } else {
                        None
                    }
                };
                if let Some(name) = selected {
                    self.load_profile(Some(name));
                }
                if !valid {
                    self.message(
                        "This configuration is not editable. Check it with lorgarctl doctor.",
                        true,
                    );
                }
            }
            Update::Config(Err(error)) => {
                self.model.borrow_mut().config_valid = false;
                self.message(&error, true);
            }
            Update::Done { result, selected } => {
                self.model.borrow_mut().busy = false;
                match result {
                    Ok(message) => {
                        if let Some(name) = selected {
                            self.load_profile((!name.is_empty()).then_some(name));
                            self.profile_name.set_text("");
                        }
                        self.message(&message, false);
                    }
                    Err(error) => self.message(&error, true),
                }
            }
            Update::Status(result) => match result {
                Ok(response) => {
                    let state = response.state.unwrap_or_default();
                    let connected = state.device.is_some();
                    self.model.borrow_mut().connected = connected;
                    let (connection, detail) = connection_copy(
                        connected,
                        state.battery_observed_ms,
                        state.last_error.as_deref(),
                        unix_time_ms(),
                    );
                    self.connection.set_text(&connection);
                    if connected {
                        self.connection.remove_css_class("pill-offline");
                        self.connection.add_css_class("pill-online");
                    } else {
                        self.connection.remove_css_class("pill-online");
                        self.connection.add_css_class("pill-offline");
                    }
                    self.battery.set_text(
                        &state
                            .battery_percentage
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "—".into()),
                    );
                    self.active_stage.set_text(
                        &state
                            .active_stage
                            .map(|s| format!("Stage {s}"))
                            .unwrap_or_else(|| "Unknown".into()),
                    );
                    self.applied
                        .set_text(state.applied_profile.as_deref().unwrap_or("None"));
                    self.connection_detail.set_text(&detail);
                }
                Err(error) => {
                    self.model.borrow_mut().connected = false;
                    self.connection.set_text("Service unavailable");
                    self.connection.remove_css_class("pill-online");
                    self.connection.add_css_class("pill-offline");
                    self.connection_detail.set_text(&error);
                    self.battery.set_text("—");
                    self.active_stage.set_text("Unknown");
                    self.applied.set_text("Unavailable");
                }
            },
        }
        self.refresh_actions();
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn connection_copy(
    device_present: bool,
    battery_observed_ms: Option<u64>,
    last_error: Option<&str>,
    now_ms: u64,
) -> (String, String) {
    if !device_present {
        return (
            "Not connected".into(),
            last_error
                .unwrap_or(
                    "Connect your mouse or receiver. Profiles can still be edited while offline.",
                )
                .into(),
        );
    }
    if let Some(error) = last_error {
        return ("Receiver available".into(), error.into());
    }
    match battery_observed_ms {
        Some(observed) => {
            let age = now_ms.saturating_sub(observed) / 1000;
            let heading = if age <= 120 {
                "State response fresh"
            } else {
                "State response stale"
            };
            (
                heading.into(),
                format!("Battery state received {age}s ago."),
            )
        }
        None => (
            "Receiver available".into(),
            "Waiting for the first state response from the mouse.".into(),
        ),
    }
}

fn confirm(
    parent: &gtk::ApplicationWindow,
    title: &str,
    text: &str,
    action_name: &str,
    action: impl Fn() + 'static,
) {
    let dialog = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .resizable(false)
        .build();
    let content = column(20);
    content.add_css_class("page");
    content.append(&label(title, "card-title"));
    let body = label(text, "muted");
    body.set_wrap(true);
    body.set_max_width_chars(45);
    content.append(&body);
    let buttons = row(12);
    buttons.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "secondary");
    let proceed = button(action_name, "danger");
    buttons.append(&cancel);
    buttons.append(&proceed);
    content.append(&buttons);
    dialog.set_child(Some(&content));
    let weak = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    let weak = dialog.downgrade();
    proceed.connect_clicked(move |_| {
        action();
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    dialog.present();
}

fn main() -> glib::ExitCode {
    let options = Options::parse();
    if let Some(path) = &options.export_model {
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir(path)?;
            let obj = std::fs::File::create_new(path.join("msa10w.obj"))?;
            mouse_model::Mesh::msa10w().write_obj(std::io::BufWriter::new(obj))?;
            let mtl = std::fs::File::create_new(path.join("msa10w.mtl"))?;
            mouse_model::Mesh::write_mtl(std::io::BufWriter::new(mtl))?;
            println!("Exported approximate visual model to {}", path.display());
            Ok(())
        })();
        return match result {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Model export failed: {error}");
                glib::ExitCode::FAILURE
            }
        };
    }
    let app = gtk::Application::builder()
        .application_id("io.github.Sqrilizz.LorgarControl")
        .build();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let css = gtk::CssProvider::new();
        css.load_from_data(include_str!("style.css"));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(true);
        }
        let (tx, rx, worker) = backend::start(options.clone());
        let ui = Ui::new(app, tx.clone(), options.experimental_3d);
        let worker = RefCell::new(Some(worker));
        app.connect_shutdown(move |_| {
            let _ = tx.send(Work::Stop);
            if let Some(worker) = worker.borrow_mut().take() {
                let _ = worker.join();
            }
        });
        let updates = ui.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            for update in rx.try_iter() {
                updates.update(update);
            }
            glib::ControlFlow::Continue
        });
        ui.window.present();
    });
    app.run_with_args::<&str>(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_copy_separates_receiver_and_state_health() {
        assert_eq!(
            connection_copy(true, None, None, 10).0,
            "Receiver available"
        );
        assert_eq!(
            connection_copy(true, Some(1_000), None, 2_000).0,
            "State response fresh"
        );
        assert_eq!(
            connection_copy(true, Some(1_000), None, 130_000).0,
            "State response stale"
        );
        assert_eq!(
            connection_copy(true, Some(1_000), Some("query timed out"), 2_000).0,
            "Receiver available"
        );
        assert_eq!(connection_copy(false, None, None, 0).0, "Not connected");
    }

    #[test]
    #[ignore = "requires a display; run with xvfb-run -a cargo test -p lorgar-app ui_smoke -- --ignored"]
    fn ui_smoke() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.Sqrilizz.LorgarControl.Tests")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ui = Ui::new(&app, tx, false);
        assert!(ui.mouse_model.is_none());
        ui.update(Update::Config(Ok(Config::default())));
        ui.polling.set_selected(4);
        ui.save.emit_clicked();
        assert_eq!(ui.stack.visible_child_name().as_deref(), Some("profiles"));
        ui.profile_name.set_text("fresh");
        ui.create.emit_clicked();
        assert!(
            matches!(rx.try_recv().unwrap(), Work::Create { name, profile } if name == "fresh" && profile.polling == Some(1000))
        );
        ui.update(Update::Done {
            result: Ok("Created".into()),
            selected: Some(String::new()),
        });
        let mut config = Config::default();
        let mut profile = Profile::default();
        profile.extra.insert("future-field".into(), true.into());
        config.profiles.insert("gaming".into(), profile.clone());
        ui.update(Update::Config(Ok(config.clone())));
        assert_eq!(ui.model.borrow().selected.as_deref(), Some("gaming"));
        assert!(!ui.apply.is_sensitive());
        for index in 0..5 {
            ui.navigate(index);
        }
        ui.dpi[0].0.set_active(true);
        ui.dpi[0].1.set_value(800.0);
        ui.polling.set_selected(4);
        ui.lighting.set_selected(1);
        ui.use_color.set_active(true);
        ui.color.set_text("ff5500");
        let draft = ui.draft().unwrap();
        assert_eq!(draft.dpi_stages[&0], 800);
        assert_eq!(draft.polling, Some(1000));
        assert_eq!(draft.color.as_deref(), Some("ff5500"));
        assert_eq!(draft.extra, profile.extra);
        assert!(ui.model.borrow().dirty);
        ui.save.emit_clicked();
        assert!(
            matches!(rx.try_recv().unwrap(), Work::Save { name, profile, .. } if name == "gaming" && profile == draft)
        );
        config.profiles.insert("gaming".into(), draft.clone());
        ui.update(Update::Config(Ok(config.clone())));
        ui.update(Update::Done {
            result: Ok("Saved".into()),
            selected: Some("gaming".into()),
        });
        assert!(!ui.model.borrow().dirty);
        ui.color.set_text("🖱️");
        assert!(ui.draft().is_err());
        ui.reset.emit_clicked();
        assert_eq!(ui.draft().unwrap(), draft);
        ui.restore.set_active(true);
        ui.rule_process.set_text("cs2");
        ui.rule_profile.set_selected(1);
        ui.add_rule.emit_clicked();
        assert!(ui.preferences_dirty());
        ui.update(Update::Config(Ok(config)));
        assert!(ui.restore.is_active());
        assert_eq!(ui.app_draft.borrow().rules[0].process, "cs2");
        ui.save_preferences.emit_clicked();
        assert!(
            matches!(rx.try_recv().unwrap(), Work::Preferences { restore: true, applications, .. } if applications.rules.len() == 1)
        );
        ui.model.borrow_mut().discard_on_close = true;
        ui.window.close();
    }
}
