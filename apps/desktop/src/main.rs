use blitzline_adapters::Store;
use blitzline_desktop::Desktop;
use blitzline_engine::Repository;
use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let data = args
        .windows(2)
        .find(|a| a[0] == "--data-dir")
        .map(|a| a[1].clone().into())
        .unwrap_or_else(Store::default_directory);
    let store = match Store::open(data) {
        Ok(s) => s,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title("Blitzline could not open")
                .set_description(e.to_string())
                .show();
            return Ok(());
        }
    };
    let state = match store.load() {
        Ok(s) => s,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title("Your saved work needs attention")
                .set_description(e.to_string())
                .show();
            return Ok(());
        }
    };
    let scenario = args
        .windows(2)
        .find(|a| a[0] == "--scenario")
        .map(|a| a[1].clone());
    let narrow = args.iter().any(|a| a == "--narrow");
    let appearance = args
        .windows(2)
        .find(|a| a[0] == "--appearance")
        .map(|a| a[1].clone());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Blitzline")
            .with_inner_size(if narrow {
                [1024.0, 768.0]
            } else {
                [1240.0, 860.0]
            })
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Blitzline",
        options,
        Box::new(move |cc| {
            let mut app = Desktop::new(Some(store), state);
            if let Some(appearance) = appearance {
                app.engine.state.preferences.appearance = match appearance.as_str() {
                    "light" => "Light",
                    "dark" => "Dark",
                    _ => "System",
                }
                .into();
            }
            app.install_platform(cc);
            if let Some(scenario) = scenario {
                app.load_scenario(&scenario);
            }
            Ok(Box::new(app))
        }),
    )
}
