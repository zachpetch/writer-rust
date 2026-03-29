// A minimal plain-text editor for macOS, built with pure Rust and Cocoa FFI.
//
// Architecture overview:
//   main()
//     -> create NSApplication
//     -> register a custom AppDelegate class with the ObjC runtime
//     -> build the menu bar (File, Edit)
//     -> create NSWindow + NSScrollView + NSTextView
//     -> [NSApp run]  (enters the Cocoa event loop — never returns)
//
// The Cocoa event loop (`[NSApp run]`) drives everything: keyboard input,
// mouse events, menu actions, window management. Menu items target either
// the app delegate (for file operations) or the first responder (for
// undo/redo, which NSTextView handles natively).

mod ffi;

use ffi::*;
use std::ffi::CString;
use std::sync::Mutex;

// --- Editor state ---

struct EditorState {
    text_view: Id,
    window: Id,
    file_path: Option<String>,
}

// Safety: Id pointers are only accessed on the main thread (Cocoa requirement).
unsafe impl Send for EditorState {}

static STATE: Mutex<EditorState> = Mutex::new(EditorState {
    text_view: NIL,
    window: NIL,
    file_path: None,
});

// --- Window title helper ---

fn update_title() {
    let st = STATE.lock().unwrap();
    if st.window == NIL {
        return;
    }
    let title = match &st.file_path {
        Some(p) => {
            // Show just the filename
            std::path::Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone())
        }
        None => "Untitled".to_string(),
    };
    let nstitle = nsstring(&title);
    unsafe {
        msg1_v(st.window, sel("setTitle:"), nstitle);
    }
}

// --- File operations ---

fn new_document() {
    {
        let st = STATE.lock().unwrap();
        if st.text_view == NIL {
            return;
        }
        unsafe {
            msg1_v(st.text_view, sel("setString:"), nsstring(""));
        }
    }
    STATE.lock().unwrap().file_path = None;
    update_title();
}

fn open_document() {
    unsafe {
        // Create and configure NSOpenPanel
        let panel = msg(cls("NSOpenPanel"), sel("openPanel"));
        msg_bool_v(panel, sel("setAllowsMultipleSelection:"), NO);
        msg_bool_v(panel, sel("setCanChooseDirectories:"), NO);
        msg_bool_v(panel, sel("setCanChooseFiles:"), YES);

        // Run modal (blocks until user picks or cancels)
        let run: extern "C" fn(Id, Sel) -> NSInteger =
            std::mem::transmute(ffi::msg_addr());
        let response = run(panel, sel("runModal"));

        // NSModalResponseOK = 1
        if response != 1 {
            return;
        }

        // Get the selected URL -> path
        let urls = msg(panel, sel("URLs"));
        let url = msg_uint(urls, sel("objectAtIndex:"), 0);
        let path_ns = msg(url, sel("path"));
        let path = nsstring_to_string(path_ns);

        // Read the file using Rust's standard library
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let st = STATE.lock().unwrap();
                msg1_v(st.text_view, sel("setString:"), nsstring(&contents));
                drop(st);
                STATE.lock().unwrap().file_path = Some(path);
                update_title();
            }
            Err(e) => {
                show_alert(&format!("Could not open file:\n{e}"));
            }
        }
    }
}

fn save_document() {
    let path = STATE.lock().unwrap().file_path.clone();
    match path {
        Some(p) => write_to_path(&p),
        None => save_document_as(),
    }
}

fn save_document_as() {
    unsafe {
        let panel = msg(cls("NSSavePanel"), sel("savePanel"));

        let run: extern "C" fn(Id, Sel) -> NSInteger =
            std::mem::transmute(ffi::msg_addr());
        let response = run(panel, sel("runModal"));

        if response != 1 {
            return;
        }

        let url = msg(panel, sel("URL"));
        let path_ns = msg(url, sel("path"));
        let path = nsstring_to_string(path_ns);

        write_to_path(&path);
        STATE.lock().unwrap().file_path = Some(path);
        update_title();
    }
}

fn write_to_path(path: &str) {
    let text = {
        let st = STATE.lock().unwrap();
        unsafe {
            let ns = msg(st.text_view, sel("string"));
            nsstring_to_string(ns)
        }
    };
    if let Err(e) = std::fs::write(path, &text) {
        show_alert(&format!("Could not save file:\n{e}"));
    }
}

fn show_alert(message: &str) {
    unsafe {
        let alert = msg(msg(cls("NSAlert"), sel("alloc")), sel("init"));
        msg1_v(alert, sel("setMessageText:"), nsstring(message));
        let _: Id = msg(alert, sel("runModal"));
    }
}

// --- App delegate (Objective-C class created at runtime) ---
//
// We register a new ObjC class "AppDelegate" that inherits from NSObject.
// We add methods for each menu action. When the user clicks a menu item,
// AppKit sends the corresponding selector to our delegate instance.

extern "C" fn delegate_new(_self: Id, _cmd: Sel, _sender: Id) {
    new_document();
}

extern "C" fn delegate_open(_self: Id, _cmd: Sel, _sender: Id) {
    open_document();
}

extern "C" fn delegate_save(_self: Id, _cmd: Sel, _sender: Id) {
    save_document();
}

extern "C" fn delegate_save_as(_self: Id, _cmd: Sel, _sender: Id) {
    save_document_as();
}

extern "C" fn delegate_should_terminate(_self: Id, _cmd: Sel, _app: Id) -> BOOL {
    YES
}

fn register_delegate_class() -> Class {
    unsafe {
        let superclass = cls("NSObject");
        let name = CString::new("AppDelegate").unwrap();
        let delegate_cls = objc_allocateClassPair(superclass, name.as_ptr(), 0);

        // Type encoding: v = void, @ = id, : = SEL
        let void_enc = CString::new("v@:@").unwrap();
        let bool_enc = CString::new("c@:@").unwrap();

        class_addMethod(
            delegate_cls,
            sel("newDocument:"),
            delegate_new as IMP,
            void_enc.as_ptr(),
        );
        class_addMethod(
            delegate_cls,
            sel("openDocument:"),
            delegate_open as IMP,
            void_enc.as_ptr(),
        );
        class_addMethod(
            delegate_cls,
            sel("saveDocument:"),
            delegate_save as IMP,
            void_enc.as_ptr(),
        );
        class_addMethod(
            delegate_cls,
            sel("saveDocumentAs:"),
            delegate_save_as as IMP,
            void_enc.as_ptr(),
        );
        class_addMethod(
            delegate_cls,
            sel("applicationShouldTerminateAfterLastWindowClosed:"),
            delegate_should_terminate as IMP,
            bool_enc.as_ptr(),
        );

        objc_registerClassPair(delegate_cls);
        delegate_cls
    }
}

// --- Menu construction ---

fn create_menu_bar(delegate: Id) {
    unsafe {
        let menu_bar = msg(msg(cls("NSMenu"), sel("alloc")), sel("init"));

        // -- Application menu --
        let app_menu_item = msg(msg(cls("NSMenuItem"), sel("alloc")), sel("init"));
        msg1_v(menu_bar, sel("addItem:"), app_menu_item);
        let app_menu = msg(msg(cls("NSMenu"), sel("alloc")), sel("init"));
        let quit_item = new_menu_item("Quit", "terminate:", "q", NIL);
        msg1_v(app_menu, sel("addItem:"), quit_item);
        msg1_v(app_menu_item, sel("setSubmenu:"), app_menu);

        // -- File menu --
        let file_menu_item = msg(msg(cls("NSMenuItem"), sel("alloc")), sel("init"));
        msg1_v(menu_bar, sel("addItem:"), file_menu_item);
        let file_menu = msg(
            msg1(
                msg(cls("NSMenu"), sel("alloc")),
                sel("initWithTitle:"),
                nsstring("File"),
            ),
            sel("autorelease"),
        );
        msg1_v(file_menu, sel("addItem:"), new_menu_item("New", "newDocument:", "n", delegate));
        msg1_v(file_menu, sel("addItem:"), new_menu_item("Open...", "openDocument:", "o", delegate));
        msg1_v(file_menu, sel("addItem:"), new_menu_item("Save", "saveDocument:", "s", delegate));
        msg1_v(file_menu, sel("addItem:"), new_menu_item("Save As...", "saveDocumentAs:", "S", delegate));
        msg1_v(file_menu_item, sel("setSubmenu:"), file_menu);

        // -- Edit menu --
        let edit_menu_item = msg(msg(cls("NSMenuItem"), sel("alloc")), sel("init"));
        msg1_v(menu_bar, sel("addItem:"), edit_menu_item);
        let edit_menu = msg(
            msg1(
                msg(cls("NSMenu"), sel("alloc")),
                sel("initWithTitle:"),
                nsstring("Edit"),
            ),
            sel("autorelease"),
        );
        // Target = NIL means first responder; NSTextView handles undo:/redo: natively.
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Undo", "undo:", "z", NIL));
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Redo", "redo:", "Z", NIL));
        // Separator
        msg1_v(
            edit_menu,
            sel("addItem:"),
            msg(cls("NSMenuItem"), sel("separatorItem")),
        );
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Cut", "cut:", "x", NIL));
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Copy", "copy:", "c", NIL));
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Paste", "paste:", "v", NIL));
        msg1_v(edit_menu, sel("addItem:"), new_menu_item("Select All", "selectAll:", "a", NIL));
        msg1_v(edit_menu_item, sel("setSubmenu:"), edit_menu);

        // Install the menu bar
        let app = msg(cls("NSApplication"), sel("sharedApplication"));
        msg1_v(app, sel("setMainMenu:"), menu_bar);
    }
}

/// Create a single NSMenuItem with title, action, key equivalent, and target.
fn new_menu_item(title: &str, action: &str, key: &str, target: Id) -> Id {
    unsafe {
        let alloc = msg(cls("NSMenuItem"), sel("alloc"));
        let init: extern "C" fn(Id, Sel, Id, Sel, Id) -> Id =
            std::mem::transmute(ffi::msg_addr());
        let item = init(
            alloc,
            sel("initWithTitle:action:keyEquivalent:"),
            nsstring(title),
            sel(action),
            nsstring(key),
        );
        if target != NIL {
            msg1_v(item, sel("setTarget:"), target);
        }
        item
    }
}

// --- Window and text view ---

fn create_window_and_editor() {
    unsafe {
        let frame = NSRect::new(100.0, 100.0, 800.0, 600.0);

        // NSWindowStyleMask: titled(1) | closable(2) | miniaturizable(4) | resizable(8)
        let style_mask: NSUInteger = 1 | 2 | 4 | 8;
        // NSBackingStoreBuffered = 2
        let backing: NSUInteger = 2;

        // Create NSWindow
        let window_alloc = msg(cls("NSWindow"), sel("alloc"));
        let init_window: extern "C" fn(Id, Sel, NSRect, NSUInteger, NSUInteger, BOOL) -> Id =
            std::mem::transmute(ffi::msg_addr());
        let window = init_window(
            window_alloc,
            sel("initWithContentRect:styleMask:backing:defer:"),
            frame,
            style_mask,
            backing,
            NO,
        );

        // Create NSScrollView
        let scroll_alloc = msg(cls("NSScrollView"), sel("alloc"));
        let init_frame: extern "C" fn(Id, Sel, NSRect) -> Id =
            std::mem::transmute(ffi::msg_addr());
        let scroll_view = init_frame(
            scroll_alloc,
            sel("initWithFrame:"),
            NSRect::new(0.0, 0.0, 800.0, 600.0),
        );
        msg_bool_v(scroll_view, sel("setHasVerticalScroller:"), YES);
        msg_bool_v(scroll_view, sel("setHasHorizontalScroller:"), NO);
        // NSBorderType: NSNoBorder = 0
        msg_uint_v(scroll_view, sel("setBorderType:"), 0);

        // Create NSTextView
        let tv_alloc = msg(cls("NSTextView"), sel("alloc"));
        let text_view = init_frame(
            tv_alloc,
            sel("initWithFrame:"),
            NSRect::new(0.0, 0.0, 800.0, 600.0),
        );

        // Configure text view for plain text editing
        msg_bool_v(text_view, sel("setEditable:"), YES);
        msg_bool_v(text_view, sel("setSelectable:"), YES);
        msg_bool_v(text_view, sel("setRichText:"), NO);
        msg_bool_v(text_view, sel("setVerticallyResizable:"), YES);
        msg_bool_v(text_view, sel("setHorizontallyResizable:"), NO);
        msg_bool_v(text_view, sel("setAllowsUndo:"), YES);

        // Auto-resize with scroll view width
        // NSViewWidthSizable = 2
        msg_uint_v(text_view, sel("setAutoresizingMask:"), 2);

        // Max size: allow unlimited vertical growth
        let set_max: extern "C" fn(Id, Sel, NSSize) =
            std::mem::transmute(ffi::msg_addr());
        set_max(
            text_view,
            sel("setMaxSize:"),
            NSSize::new(1.0e7, 1.0e7),
        );
        let set_min: extern "C" fn(Id, Sel, NSSize) =
            std::mem::transmute(ffi::msg_addr());
        set_min(text_view, sel("setMinSize:"), NSSize::new(0.0, 0.0));

        // Text container: track scroll view width
        let container = msg(text_view, sel("textContainer"));
        let set_container_size: extern "C" fn(Id, Sel, NSSize) =
            std::mem::transmute(ffi::msg_addr());
        set_container_size(
            container,
            sel("setContainerSize:"),
            NSSize::new(1.0e7, 1.0e7),
        );
        msg_bool_v(container, sel("setWidthTracksTextView:"), YES);

        // Monospace font: [NSFont monospacedSystemFontOfSize:14 weight:0]
        let font_msg: extern "C" fn(Id, Sel, CGFloat, CGFloat) -> Id =
            std::mem::transmute(ffi::msg_addr());
        let font = font_msg(
            cls("NSFont"),
            sel("monospacedSystemFontOfSize:weight:"),
            14.0,
            0.0, // NSFontWeightRegular
        );
        msg1_v(text_view, sel("setFont:"), font);

        // Assemble: text view -> scroll view -> window
        msg1_v(scroll_view, sel("setDocumentView:"), text_view);
        msg1_v(window, sel("setContentView:"), scroll_view);

        // Store references
        {
            let mut st = STATE.lock().unwrap();
            st.text_view = text_view;
            st.window = window;
        }

        // Show the window
        msg_bool_v(window, sel("makeKeyAndOrderFront:"), NO);
    }

    update_title();
}

// --- Entry point ---

fn main() {
    unsafe {
        // Create the shared NSApplication instance.
        // This is the Cocoa application object that owns the event loop.
        let app = msg(cls("NSApplication"), sel("sharedApplication"));

        // NSApplicationActivationPolicyRegular = 0
        // Makes this a proper GUI app with a dock icon and menu bar.
        msg_uint_v(app, sel("setActivationPolicy:"), 0);

        // Register our custom ObjC delegate class and instantiate it
        let delegate_cls = register_delegate_class();
        let delegate = msg(msg(delegate_cls, sel("alloc")), sel("init"));
        msg1_v(app, sel("setDelegate:"), delegate);

        // Build menus, window, and text view
        create_menu_bar(delegate);
        create_window_and_editor();

        // Bring the app to the foreground
        msg_bool_v(app, sel("activateIgnoringOtherApps:"), YES);

        // Enter the Cocoa event loop. This call never returns.
        // From here on, macOS dispatches events (key presses, mouse clicks,
        // menu selections, window events) to the appropriate responders.
        // Our delegate callbacks are invoked when the user triggers menu actions.
        msg_v(app, sel("run"));
    }
}
