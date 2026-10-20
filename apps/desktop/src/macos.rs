//! Real AppKit chrome. All Objective-C interaction stays on the main thread.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSCopying, NSObject, NSObjectProtocol, NSString,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Projects,
    New,
    Open,
    Import,
    Larger,
    Smaller,
    Settings,
    Appearance,
    Help,
}

#[derive(Default)]
pub struct Ivars {
    commands: RefCell<Vec<Command>>,
}

define_class!(
    // NSObject has no subclassing invariants; callbacks run on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct ToolbarDelegate;
    unsafe impl NSObjectProtocol for ToolbarDelegate {}
    unsafe impl NSToolbarDelegate for ToolbarDelegate {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        #[unsafe(method_family = none)]
        fn defaults(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            identifiers()
        }
        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        #[unsafe(method_family = none)]
        fn allowed(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            identifiers()
        }
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        #[unsafe(method_family = none)]
        fn item(&self, _toolbar: &NSToolbar, identifier: &NSString, _insert: bool) -> Option<Retained<NSToolbarItem>> {
            (|| {
            let (label, symbol, action) = match identifier.to_string().as_str() {
                "projects" => ("Projects", "square.grid.2x2", sel!(projects:)),
                "new" => ("New project", "plus", sel!(newProject:)),
                "settings" => ("Settings", "gearshape", sel!(settings:)),
                "appearance" => ("Switch light or dark appearance", "circle.lefthalf.filled", sel!(appearance:)),
                "help" => ("Help", "questionmark.circle", sel!(help:)),
                _ => return None,
            };
            let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(self.mtm()), identifier);
            let label = NSString::from_str(label);
            item.setLabel(&label);
            item.setPaletteLabel(&label);
            item.setToolTip(Some(&label));
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), Some(&label));
            item.setImage(image.as_deref());
            // Target is retained by NativeChrome for the toolbar's entire lifetime.
            unsafe {
                item.setTarget(Some(self));
                item.setAction(Some(action));
            }
            Some(item)
            })()
        }
    }
    impl ToolbarDelegate {
        #[unsafe(method(openFiles:))]
        fn open_files(&self, _sender: &NSObject) { self.push(Command::Open); }
        #[unsafe(method(importSettings:))]
        fn import_settings(&self, _sender: &NSObject) { self.push(Command::Import); }
        #[unsafe(method(larger:))]
        fn larger(&self, _sender: &NSObject) { self.push(Command::Larger); }
        #[unsafe(method(smaller:))]
        fn smaller(&self, _sender: &NSObject) { self.push(Command::Smaller); }
        #[unsafe(method(projects:))]
        fn projects(&self, _sender: &NSObject) { self.push(Command::Projects); }
        #[unsafe(method(newProject:))]
        fn new_project(&self, _sender: &NSObject) { self.push(Command::New); }
        #[unsafe(method(settings:))]
        fn settings(&self, _sender: &NSObject) { self.push(Command::Settings); }
        #[unsafe(method(appearance:))]
        fn appearance(&self, _sender: &NSObject) { self.push(Command::Appearance); }
        #[unsafe(method(help:))]
        fn help(&self, _sender: &NSObject) { self.push(Command::Help); }
    }
);
impl ToolbarDelegate {
    fn push(&self, command: Command) {
        self.ivars().commands.borrow_mut().push(command);
    }
}
fn identifiers() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&[
        NSString::from_str("projects"),
        NSString::from_str("new"),
        // AppKit owns spacing, overflow, focus, and item layout.
        unsafe { NSToolbarFlexibleSpaceItemIdentifier }.copy(),
        NSString::from_str("appearance"),
        NSString::from_str("settings"),
        NSString::from_str("help"),
    ])
}

pub struct NativeChrome {
    window: Retained<NSWindow>,
    toolbar: Retained<NSToolbar>,
    delegate: Retained<ToolbarDelegate>,
    menu_items: Vec<(Retained<NSMenu>, Retained<NSMenuItem>)>,
}
impl NativeChrome {
    pub fn report_capture_window(&self) {
        if let Ok(title) = std::env::var("BLITZLINE_CAPTURE_WINDOW_TITLE") {
            for window in NSApplication::sharedApplication(self.delegate.mtm()).windows() {
                if window.title().to_string() == title && window.isVisible() {
                    eprintln!("native-capture-id={}", window.windowNumber());
                }
            }
        }
    }
    pub fn install(handle: &impl HasWindowHandle) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(raw) = handle.window_handle().ok()?.as_raw() else {
            return None;
        };
        // winit supplies a live NSView. Borrow it only while the window handle is live.
        let view = unsafe { &*raw.ns_view.as_ptr().cast::<NSView>() };
        let window = view.window()?;
        let allocated = ToolbarDelegate::alloc(mtm).set_ivars(Ivars::default());
        let delegate: Retained<ToolbarDelegate> = unsafe { msg_send![super(allocated), init] };
        let inspect = std::env::var_os("BLITZLINE_NATIVE_INSPECT").is_some();
        let toolbar = NSToolbar::initWithIdentifier(
            NSToolbar::alloc(mtm),
            &NSString::from_str(if inspect {
                "Blitzline.VisualCheck"
            } else {
                "Blitzline.Main"
            }),
        );
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        toolbar.setAllowsUserCustomization(true);
        toolbar.setAutosavesConfiguration(!inspect);
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        window.setToolbar(Some(&toolbar));
        window.setTitle(&NSString::from_str("Blitzline"));
        if inspect {
            eprintln!("native-window-id={}", window.windowNumber());
            eprintln!("native-toolbar-items={}", toolbar.items().count());
            let app = NSApplication::sharedApplication(mtm);
            let mut checked = 0;
            for item in toolbar.items() {
                let expected = match item.itemIdentifier().to_string().as_str() {
                    "projects" => Command::Projects,
                    "new" => Command::New,
                    "appearance" => Command::Appearance,
                    "settings" => Command::Settings,
                    "help" => Command::Help,
                    _ => continue,
                };
                // Exercise AppKit target/action dispatch without running application
                // commands. This is only enabled by the native visual-check script.
                unsafe {
                    assert!(app.sendAction_to_from(
                        item.action().unwrap(),
                        item.target().as_deref(),
                        Some(&item)
                    ));
                }
                assert_eq!(delegate.ivars().commands.take(), vec![expected]);
                checked += 1;
            }
            assert_eq!(checked, 5);
            eprintln!("native-toolbar-actions-verified={checked}");
        }
        let mut menu_items = Vec::new();
        if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
            if let Some(app_menu) = menu.itemAtIndex(0).and_then(|item| item.submenu()) {
                let item = unsafe {
                    let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str("Settings…"),
                        Some(sel!(settings:)),
                        &NSString::from_str(","),
                    );
                    item.setTarget(Some(&delegate));
                    item
                };
                app_menu.insertItem_atIndex(&item, 2);
                menu_items.push((app_menu, item));
            }
            for (title, entries) in [
                (
                    "File",
                    vec![
                        ("New Project", sel!(newProject:), "n"),
                        ("Choose Files…", sel!(openFiles:), "o"),
                        ("Import Python Settings…", sel!(importSettings:), ""),
                    ],
                ),
                (
                    "View",
                    vec![
                        ("Projects", sel!(projects:), ""),
                        ("Switch Light / Dark", sel!(appearance:), ""),
                        ("Larger Text", sel!(larger:), "+"),
                        ("Smaller Text", sel!(smaller:), "-"),
                    ],
                ),
                ("Help", vec![("Blitzline Help", sel!(help:), "?")]),
            ] {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(title));
                let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
                for (label, action, key) in entries {
                    // Each selector is implemented above; delegate outlives these menu items.
                    let child = unsafe {
                        let child = NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(label),
                            Some(action),
                            &NSString::from_str(key),
                        );
                        child.setTarget(Some(&delegate));
                        child
                    };
                    submenu.addItem(&child);
                }
                item.setSubmenu(Some(&submenu));
                menu.addItem(&item);
                menu_items.push((menu.clone(), item));
            }
        }
        Some(Self {
            window,
            toolbar,
            delegate,
            menu_items,
        })
    }
    pub fn commands(&self) -> Vec<Command> {
        self.delegate.ivars().commands.take()
    }
    pub fn appearance(&self, preference: &str) {
        let mtm = self.delegate.mtm();
        let name = match preference {
            "Light" => Some(unsafe { NSAppearanceNameAqua }),
            "Dark" => Some(unsafe { NSAppearanceNameDarkAqua }),
            _ => None,
        };
        let appearance = name.and_then(NSAppearance::appearanceNamed);
        NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
    }
}
impl Drop for NativeChrome {
    fn drop(&mut self) {
        for (menu, item) in &self.menu_items {
            menu.removeItem(item);
        }
        self.window.setToolbar(None);
        self.toolbar.setDelegate(None);
    }
}
