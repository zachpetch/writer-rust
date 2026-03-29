# Writer (Rust)

A minimal plain-text editor for macOS, built with pure Rust and no external crates. All GUI functionality is achieved through direct FFI bindings to the Objective-C runtime and macOS Cocoa frameworks (AppKit, Foundation).

## Build and Run

Requires a Mac with Rust installed.
Built on macOS 26.4 with Rust 1.94.0, if it matters (I hope it doesn't, much).

```bash
cargo build          # compile
cargo run            # launch the editor
```

No additional dependencies or setup needed — the project links against system frameworks only.

## Features

- Single-window plain-text editor using `NSTextView`
- Monospace system font (14pt)
- File menu: New, Open, Save, Save As
- Edit menu: Undo, Redo, Cut, Copy, Paste, Select All
- Native macOS file dialogs (`NSOpenPanel` / `NSSavePanel`)
- Standard keyboard shortcuts:

| Action   | Shortcut       |
|----------|----------------|
| New      | Cmd+N          |
| Open     | Cmd+O          |
| Save     | Cmd+S          |
| Save As  | Cmd+Shift+S    |
| Undo     | Cmd+Z          |
| Redo     | Cmd+Shift+Z    |
| Quit     | Cmd+Q          |

## How It Works

### Objective-C Messaging

macOS GUI APIs are Objective-C classes. To call them from Rust, the code uses three runtime functions:

- `objc_getClass` — look up a class by name (e.g., `"NSWindow"`)
- `sel_registerName` — look up a selector by name (e.g., `"alloc"`)
- `objc_msgSend` — send a message (call a method) on an object

`objc_msgSend` is a trampoline with no fixed signature. At each call site, its address is `transmute`d to the exact function pointer type the method expects:

```rust
// Equivalent to: [NSWindow alloc]
let f: extern "C" fn(Id, Sel) -> Id = transmute(msg_addr());
f(cls("NSWindow"), sel("alloc"))
```

### Event Loop

`[NSApp run]` enters the Cocoa run loop, which dispatches all events (keyboard, mouse, menu selections, window management) to the appropriate responders. Menu item clicks invoke selectors on their target — either the app delegate (for file operations) or the first responder via a `nil` target (for undo/redo, cut/copy/paste).

### NSTextView Integration

`NSTextView` is wrapped in an `NSScrollView` and set as the window's content view. It provides all text editing behavior out of the box, including built-in undo/redo through its `NSUndoManager`. The text view is configured as plain-text only (`setRichText: NO`) with vertical resizing and width tracking.

### App Delegate

A custom Objective-C class (`AppDelegate`) is registered at runtime using `objc_allocateClassPair` and `class_addMethod`. This class implements the menu action selectors (`newDocument:`, `openDocument:`, etc.) as `extern "C"` Rust functions.

### File I/O

Native macOS dialogs handle file selection. Actual reading and writing uses `std::fs::read_to_string` and `std::fs::write`.

## Project Structure

```
src/
  ffi.rs    — Objective-C runtime bindings, geometry types, objc_msgSend wrappers,
              NSString conversion helpers
  main.rs   — Editor state, file operations, delegate class registration, menu
              construction, window/text view setup, entry point
```

## Limitations

- No dirty-flag tracking — closing with unsaved changes does not prompt to save.
- No title bar edit indicator (the dot in the close button for modified documents).
- Single document only — no tabs or multiple windows.
- x86_64 compatibility — struct-returning `objc_msgSend` calls (`objc_msgSend_stret`) are not used in this code but would be needed if calling methods that return `NSRect` on Intel Macs.
- No explicit `NSAutoreleasePool` during setup — the run loop manages autorelease during event handling, but setup-phase Objective-C objects could theoretically leak small amounts of memory.

## Possible Improvements

- Prompt to save unsaved changes on close (dirty flag + `windowShouldClose:` delegate method).
- Recent files menu.
- Line and column number display in a status bar.
- Find and replace via `NSTextFinder`.
- Syntax highlighting or line numbering (would require more extensive `NSTextStorage` work).
- Proper `NSAutoreleasePool` wrapping during initialization.
