use eframe::egui;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use sysinfo::System;

#[derive(Serialize, Deserialize, Clone)]
struct Profile {
    id: String,
    name: String,
    exe: String,
    args: Vec<String>,
}

fn cfg_file() -> PathBuf {
    let p = directories::ProjectDirs::from("", "", "DirectMCLaunch").unwrap();
    fs::create_dir_all(p.data_dir()).ok();
    p.data_dir().join("profiles.json")
}

fn load() -> Vec<Profile> {
    fs::read_to_string(cfg_file()).map(|s| serde_json::from_str(&s).unwrap_or_default()).unwrap_or_default()
}

fn save(p: &[Profile]) {
    fs::write(cfg_file(), serde_json::to_string(p).unwrap()).ok();
}

fn run_profile_async(id: &str, done: Arc<AtomicBool>) {
    if let Some(p) = load().into_iter().find(|x| x.id == id) {
        std::thread::spawn(move || {
            let mut c = Command::new(&p.exe);
            c.args(&p.args);
            c.stdout(std::process::Stdio::piped());
            c.stderr(std::process::Stdio::piped());
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x08000000);
            }
            if let Ok(mut child) = c.spawn() {
                if let Some(stdout) = child.stdout.take() {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().flatten() {
                        let l = line.to_lowercase();
                        if l.contains("lwjgl") || l.contains("opengl") || l.contains("setting user") || l.contains("backend library") {
                            break;
                        }
                    }
                }
            }
            done.store(true, Ordering::SeqCst);
        });
    } else {
        done.store(true, Ordering::SeqCst);
    }
}

fn scan_mc() -> Option<Profile> {
    let mut s = System::new_all();
    s.refresh_all();
    for (_pid, proc) in s.processes() {
        let name = proc.name().to_string_lossy().to_lowercase();
        if name.contains("java") {
            let cmd = proc.cmd();
            let cmd_str = cmd.iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>();
            if cmd_str.iter().any(|x| x.contains("minecraft") || x.contains("fabric") || x.contains("forge") || x.contains("quilt")) {
                let exe = proc.exe().map(|p| p.to_string_lossy().to_string()).unwrap_or(cmd_str[0].clone());
                let args = cmd_str.into_iter().skip(1).collect();
                return Some(Profile {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Minecraft".to_string(),
                    exe,
                    args,
                });
            }
        }
    }
    None
}

fn make_shortcut(p: &Profile) {
    let self_exe = env::current_exe().unwrap();
    #[cfg(target_os = "macos")]
    {
        let app = PathBuf::from(env::var("HOME").unwrap()).join(format!("Applications/{}.app", p.name));
        let macos = app.join("Contents/MacOS");
        let res = app.join("Contents/Resources");
        fs::create_dir_all(&macos).ok();
        fs::create_dir_all(&res).ok();
        let sh = macos.join(&p.name);
        fs::write(&sh, format!("#!/bin/sh\n\"{}\" --run {}", self_exe.display(), p.id)).ok();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sh, fs::Permissions::from_mode(0o755)).ok();
        fs::write(res.join("AppIcon.icns"), include_bytes!("../assets/mc.icns")).ok();
        let plist = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleName</key>
    <string>{}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
</dict>
</plist>"#, p.name, p.name);
        fs::write(app.join("Contents/Info.plist"), plist).ok();
    }
    #[cfg(target_os = "windows")]
    {
        let lnk = PathBuf::from(env::var("USERPROFILE").unwrap()).join(format!("Desktop\\{}.lnk", p.name));
        let p_dir = directories::ProjectDirs::from("", "", "DirectMCLaunch").unwrap();
        let ico_path = p_dir.data_dir().join("mc.ico");
        fs::write(&ico_path, include_bytes!("../assets/mc.ico")).ok();
        if let Ok(mut sl) = mslnk::ShellLink::new(self_exe.to_str().unwrap()) {
            sl.set_arguments(Some(format!("--run {}", p.id)));
            sl.set_icon_location(Some(ico_path.to_str().unwrap().to_string()));
            sl.create_lnk(&lnk).ok();
        }
    }
    #[cfg(target_os = "linux")]
    {
        let dk = PathBuf::from(env::var("HOME").unwrap()).join(format!(".local/share/applications/{}.desktop", p.name));
        let p_dir = directories::ProjectDirs::from("", "", "DirectMCLaunch").unwrap();
        let ico_path = p_dir.data_dir().join("mc.png");
        fs::write(&ico_path, include_bytes!("../assets/mc_real.png")).ok();
        let c = format!("[Desktop Entry]\nName={}\nExec=\"{}\" --run {}\nType=Application\nTerminal=false\nIcon={}\n", p.name, self_exe.display(), p.id, ico_path.display());
        fs::write(&dk, c).ok();
    }
}

struct App {
    profs: Vec<Profile>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("DirectMCLaunch").size(24.0).strong());
            });
            ui.add_space(10.0);
            
            let mut do_save = false;
            if ui.button("Scan Running Minecraft").clicked() {
                if let Some(mut p) = scan_mc() {
                    p.name = format!("MC {}", self.profs.len() + 1);
                    self.profs.push(p);
                    do_save = true;
                }
            }
            ui.add_space(10.0);
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut del = None;
                let mut make_sc = None;
                for (i, p) in self.profs.iter_mut().enumerate() {
                    egui::Frame::group(ui.style()).rounding(8.0).inner_margin(12.0).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Name:").strong());
                            ui.text_edit_singleline(&mut p.name);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Delete").clicked() { del = Some(i); }
                                if ui.button("OS Shortcut").clicked() { make_sc = Some(p.clone()); }
                                if ui.button("Save").clicked() { do_save = true; }
                            });
                        });
                        
                        if let Some(a) = p.args.iter_mut().find(|x| x.starts_with("-Xmx")) {
                            let mut r = a.replace("-Xmx", "");
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("RAM:").strong());
                                if ui.text_edit_singleline(&mut r).changed() {
                                    *a = format!("-Xmx{}", r);
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = del {
                    self.profs.remove(i);
                    do_save = true;
                }
                if let Some(p) = make_sc {
                    make_shortcut(&p);
                }
                if do_save {
                    save(&self.profs);
                }
            });
        });
    }
}

struct LoadingApp {
    done: Arc<AtomicBool>,
    start: std::time::Instant,
}

impl eframe::App for LoadingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.done.load(Ordering::SeqCst) || self.start.elapsed().as_secs() > 30 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.0 - 25.0);
                ui.spinner();
                ui.add_space(10.0);
                ui.heading("Starting Minecraft...");
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn load_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/app_icon.png")).unwrap().to_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}

fn load_mc_icon() -> egui::IconData {
    egui::IconData { rgba: vec![], width: 1, height: 1 }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 3 && args[1] == "--run" {
        let done = Arc::new(AtomicBool::new(false));
        run_profile_async(&args[2], done.clone());
        let opt = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 100.0])
                .with_decorations(true)
                .with_title("Loading")
                .with_icon(load_mc_icon()),
            ..Default::default()
        };
        eframe::run_native("Loading", opt, Box::new(move |_cc| Ok(Box::new(LoadingApp { done, start: std::time::Instant::now() })))).ok();
        return;
    }
    
    let opt = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(load_icon()),
        ..Default::default()
    };
    eframe::run_native("DirectMCLaunch", opt, Box::new(|_cc| Ok(Box::new(App { profs: load() })))).ok();
}
