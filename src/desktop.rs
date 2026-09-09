//! macOS shortcut and menu-bar integration. All AppKit objects are created on
//! the UI thread; callbacks only send events and wake egui.
use eframe::egui;
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use std::sync::mpsc;
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

pub enum Event {
    Shortcut(HotKeyState),
    Record,
    Quit,
}
pub struct Desktop {
    pub receiver: mpsc::Receiver<Event>,
    pub shortcut_ready: bool,
    pub tray_ready: bool,
    _hotkeys: Option<GlobalHotKeyManager>,
    tray: Option<TrayIcon>,
}
impl Desktop {
    pub fn new(ctx: &egui::Context) -> (Self, Option<String>) {
        let (sender, receiver) = mpsc::channel();
        let mut errors = Vec::new();
        let hotkeys = GlobalHotKeyManager::new().and_then(|manager| {
            manager.register(HotKey::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::Space,
            ))?;
            Ok(manager)
        });
        let hotkeys = match hotkeys {
            Ok(h) => Some(h),
            Err(e) => {
                errors.push(format!("Could not register ⌘⇧Space: {e}"));
                None
            }
        };
        let keys = sender.clone();
        let wake = ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = keys.send(Event::Shortcut(event.state));
            wake.request_repaint();
        }));
        let menu = Menu::new();
        let show = MenuItem::with_id("show", "Show FastAF Flow", true, None);
        let record = MenuItem::with_id("record", "Start / finish dictation", true, None);
        let quit = MenuItem::with_id("quit", "Quit FastAF Flow", true, None);
        let _ = menu.append_items(&[&show, &record, &PredefinedMenuItem::separator(), &quit]);
        let wake = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            match event.id.0.as_str() {
                "show" => {
                    wake.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    wake.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    wake.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                "record" => {
                    let _ = sender.send(Event::Record);
                }
                "quit" => {
                    let _ = sender.send(Event::Quit);
                }
                _ => {}
            }
            wake.request_repaint();
        }));
        let mut rgba = vec![0u8; 32 * 32 * 4];
        for (x, height) in [(5, 8), (10, 18), (15, 26), (20, 14), (25, 6)] {
            for y in (32 - height) / 2..(32 + height) / 2 {
                for dx in 0..3 {
                    rgba[(y * 32 + x + dx) * 4 + 3] = 255;
                }
            }
        }
        let tray =
            Icon::from_rgba(rgba, 32, 32).ok().and_then(|icon| {
                match TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_icon(icon)
                    .with_icon_as_template(true)
                    .with_tooltip("FastAF Flow · ⌘⇧Space")
                    .build()
                {
                    Ok(t) => Some(t),
                    Err(e) => {
                        errors.push(format!("Menu bar unavailable: {e}"));
                        None
                    }
                }
            });
        (
            Self {
                receiver,
                shortcut_ready: hotkeys.is_some(),
                tray_ready: tray.is_some(),
                _hotkeys: hotkeys,
                tray,
            },
            if errors.is_empty() {
                None
            } else {
                Some(errors.join(". "))
            },
        )
    }
    pub fn recording(&self, recording: bool) {
        if let Some(tray) = &self.tray {
            tray.set_title(if recording { Some("●") } else { None });
        }
    }
}
