//! Application logic: AppDelegate, SpacesView (custom NSView), status item,
//! workspace notifications, and top-level refresh coordination.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBezierPath, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSStringDrawing,
    NSView, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};

use crate::skylight::{self, SLSpace};

// On 64-bit Apple platforms CGFloat is f64.
type CGFloat = f64;

// ── Layout constants ──────────────────────────────────────────────────────────

const BUTTON_W: CGFloat = 24.0;
const BUTTON_H: CGFloat = 15.0;
const BUTTON_PADDING: CGFloat = 5.0;
const SEPARATOR_W: CGFloat = 8.0;
const CORNER_RADIUS: CGFloat = 6.0;
const BORDER_WIDTH: CGFloat = 1.0;

// ── Shared state ──────────────────────────────────────────────────────────────

pub struct AppState {
    pub spaces: Vec<SLSpace>,
    pub show_display_separator: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            spaces: Vec::new(),
            show_display_separator: true,
        }
    }
}

// ── SpacesView ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SpacesViewIvars {
    state: RefCell<Option<Arc<Mutex<AppState>>>>,
}

define_class!(
    /// Custom NSView that draws a row of space indicator buttons.
    ///
    /// Each button is a rounded rectangle:
    ///   - Active (has focus):  filled, inverted label
    ///   - Visible (on display, not focused): filled inner rect, label at 0.7 alpha
    ///   - Inactive: outline only, label at 0.5 alpha
    ///
    /// Left-clicking a button calls `yabai -m space --focus <index>`.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SpacesViewIvars]
    struct SpacesView;

    unsafe impl NSObjectProtocol for SpacesView {}

    impl SpacesView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // Use top-down drawing coordinates.
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            // Fill the entire bounds with black first, so every redraw
            // starts from a clean slate regardless of previous state.
            NSColor::blackColor().setFill();
            NSBezierPath::fillRect(self.bounds());

            let state_opt = self.ivars().state.borrow();
            let Some(state_arc) = state_opt.as_ref() else {
                return;
            };
            let state = state_arc.lock().expect("state poisoned");
            draw_spaces(&state);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &objc2_app_kit::NSEvent) {
            let loc = event.locationInWindow();
            let local = self.convertPoint_fromView(loc, None);

            let state_opt = self.ivars().state.borrow();
            let Some(state_arc) = state_opt.as_ref() else {
                return;
            };
            let state = state_arc.lock().expect("state poisoned");
            let mut x: CGFloat = 0.0;
            let mut last_display: Option<usize> = None;

            for (label_idx, space) in state.spaces.iter().enumerate() {
                if state.show_display_separator
                    && last_display.is_some_and(|d| d != space.display_index)
                {
                    x += SEPARATOR_W;
                }
                last_display = Some(space.display_index);

                let rect = NSRect::new(
                    NSPoint::new(x + BUTTON_PADDING / 2.0, BUTTON_PADDING / 2.0),
                    NSSize::new(BUTTON_W, BUTTON_H),
                );

                if point_in_rect(local, rect) {
                    // Focus by 1-based space index (position in the list).
                    let idx = (label_idx + 1) as i64;
                    drop(state);
                    crate::yabai::focus_space(idx);
                    return;
                }
                x += BUTTON_W + BUTTON_PADDING;
            }
        }
    }
);

impl SpacesView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SpacesViewIvars::default());
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(0.0, BUTTON_H + BUTTON_PADDING),
        );
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn set_state(&self, state: Arc<Mutex<AppState>>) {
        *self.ivars().state.borrow_mut() = Some(state);
        self.resize_to_fit();
    }

    fn resize_to_fit(&self) {
        let width = self.content_width();
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, BUTTON_H + BUTTON_PADDING),
        );
        self.setFrame(frame);
        self.setNeedsDisplay(true);
    }

    fn content_width(&self) -> CGFloat {
        let state_opt = self.ivars().state.borrow();
        let Some(state_arc) = state_opt.as_ref() else {
            return BUTTON_W + BUTTON_PADDING;
        };
        let state = state_arc.lock().expect("state poisoned");
        spaces_width(&state.spaces, state.show_display_separator)
    }
}

// ── Drawing helpers ───────────────────────────────────────────────────────────

fn draw_spaces(state: &AppState) {
    let mut x: CGFloat = 0.0;
    let mut last_display: Option<usize> = None;

    for (label_idx, space) in state.spaces.iter().enumerate() {
        if state.show_display_separator {
            if let Some(ld) = last_display {
                if ld != space.display_index {
                    draw_separator(x);
                    x += SEPARATOR_W;
                }
            }
        }
        last_display = Some(space.display_index);

        let rect = NSRect::new(
            NSPoint::new(x + BUTTON_PADDING / 2.0, BUTTON_PADDING / 2.0),
            NSSize::new(BUTTON_W, BUTTON_H),
        );
        // Label is 1-based sequential index across all displays.
        draw_button(rect, space, label_idx + 1);
        x += BUTTON_W + BUTTON_PADDING;
    }
}

fn draw_button(rect: NSRect, space: &SLSpace, label: usize) {
    let label = if space.is_fullscreen {
        "F".to_string()
    } else {
        label.to_string()
    };
    let path =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, CORNER_RADIUS, CORNER_RADIUS);

    if space.is_active {
        // Fill white for active.
        NSColor::whiteColor().setFill();
        path.fill();
    }

    // White border on every button, active or not.
    NSColor::whiteColor().setStroke();
    path.setLineWidth(BORDER_WIDTH);
    path.stroke();

    let text_color = if space.is_active {
        NSColor::blackColor()
    } else {
        NSColor::whiteColor()
    };
    draw_label_centered(&label, rect, &text_color);
}

fn draw_separator(x: CGFloat) {
    NSColor::labelColor().setStroke();
    let mid_x = x + SEPARATOR_W / 2.0;
    let total_h = BUTTON_H + BUTTON_PADDING;
    let margin = 4.0_f64;
    NSBezierPath::strokeLineFromPoint_toPoint(
        NSPoint::new(mid_x, margin),
        NSPoint::new(mid_x, total_h - margin),
    );
}

/// Draw `text` centered in `rect`.
/// `inverted = true` → label colour opposite to `NSColor::labelColor()` (for
/// filled active buttons). `alpha` is applied to the label colour.
fn draw_label_centered(text: &str, rect: NSRect, color: &NSColor) {
    use objc2::runtime::ProtocolObject;
    use objc2_foundation::{NSCopying, NSMutableDictionary};

    unsafe {
        let font = NSFont::systemFontOfSize(10.0);
        let ns_text = NSString::from_str(text);

        let dict: Retained<NSMutableDictionary<NSString, AnyObject>> = NSMutableDictionary::new();
        dict.setObject_forKey(
            font.as_ref(),
            ProtocolObject::<dyn NSCopying>::from_ref(NSFontAttributeName),
        );
        dict.setObject_forKey(
            color,
            ProtocolObject::<dyn NSCopying>::from_ref(NSForegroundColorAttributeName),
        );

        let size = ns_text.sizeWithAttributes(Some(&*dict));
        let draw_pt = NSPoint::new(
            rect.origin.x + (rect.size.width - size.width) / 2.0,
            rect.origin.y + (rect.size.height - size.height) / 2.0,
        );
        ns_text.drawAtPoint_withAttributes(draw_pt, Some(&*dict));
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn spaces_width(spaces: &[SLSpace], show_separator: bool) -> CGFloat {
    if spaces.is_empty() {
        return BUTTON_W + BUTTON_PADDING;
    }
    let mut w: CGFloat = 0.0;
    let mut last_display: Option<usize> = None;
    for space in spaces {
        if show_separator && last_display.is_some_and(|d| d != space.display_index) {
            w += SEPARATOR_W;
        }
        last_display = Some(space.display_index);
        w += BUTTON_W + BUTTON_PADDING;
    }
    w
}

fn point_in_rect(p: NSPoint, r: NSRect) -> bool {
    p.x >= r.origin.x
        && p.x <= r.origin.x + r.size.width
        && p.y >= r.origin.y
        && p.y <= r.origin.y + r.size.height
}

// ── AppDelegate ───────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppDelegateIvars {
    status_item: RefCell<Option<Retained<NSStatusItem>>>,
    spaces_view: RefCell<Option<Retained<SpacesView>>>,
    state: RefCell<Option<Arc<Mutex<AppState>>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::new().unwrap();
            self.setup(mtm);
        }
    }

    impl AppDelegate {
        #[unsafe(method(refresh))]
        fn refresh_sel(&self) {
            // SkyLight is always current — no delay or retry needed.
            self.do_refresh();
        }

        #[unsafe(method(toggleSeparator:))]
        fn toggle_separator(&self, _sender: &AnyObject) {
            {
                let state_opt = self.ivars().state.borrow();
                if let Some(state_arc) = state_opt.as_ref() {
                    let mut state = state_arc.lock().expect("state poisoned");
                    state.show_display_separator = !state.show_display_separator;
                }
            }
            self.update_view_size();
            self.rebuild_menu();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::new().unwrap();
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    /// Perform a full refresh: read space state from SkyLight, update view.
    pub fn do_refresh(&self) {
        let spaces = skylight::query_spaces();
        if let Some(state_arc) = self.ivars().state.borrow().as_ref() {
            let mut state = state_arc.lock().expect("state poisoned");
            state.spaces = spaces;
        }
        self.update_view_size();
        self.rebuild_menu();
    }

    fn update_view_size(&self) {
        let view_borrow = self.ivars().spaces_view.borrow();
        let item_borrow = self.ivars().status_item.borrow();
        if let Some(view) = view_borrow.as_ref() {
            view.resize_to_fit();
            if let Some(item) = item_borrow.as_ref() {
                item.setLength(view.content_width());
            }
        }
    }

    fn setup(&self, mtm: MainThreadMarker) {
        let state = Arc::new(Mutex::new(AppState::new()));

        let status_bar = NSStatusBar::systemStatusBar();
        let item = status_bar.statusItemWithLength(BUTTON_W + BUTTON_PADDING);

        let view = SpacesView::new(mtm);
        view.set_state(Arc::clone(&state));

        if let Some(button) = item.button(mtm) {
            button.addSubview(&view);
            view.setFrameOrigin(NSPoint::new(0.0, 0.0));
        }

        *self.ivars().status_item.borrow_mut() = Some(item);
        *self.ivars().spaces_view.borrow_mut() = Some(view);
        *self.ivars().state.borrow_mut() = Some(Arc::clone(&state));

        self.register_observers();
        self.start_sockets();
        self.do_refresh();
    }

    fn register_observers(&self) {
        let workspace = NSWorkspace::sharedWorkspace();
        let nc = workspace.notificationCenter();
        unsafe {
            nc.addObserver_selector_name_object(
                self,
                objc2::sel!(refresh),
                Some(NSWorkspaceActiveSpaceDidChangeNotification),
                None,
            );
        }
    }

    fn start_sockets(&self) {
        use crate::socket;

        // SAFETY: AppDelegate lives for the entire program lifetime and is only
        // touched on the main thread. The closure dispatches work to the main
        // queue before touching the pointer.
        let ptr = self as *const AppDelegate as usize;

        let make_refresh = move || {
            DispatchQueue::main().exec_async(move || {
                let delegate = unsafe { &*(ptr as *const AppDelegate) };
                delegate.do_refresh();
            });
        };

        socket::start(socket::SOCKET_PATH, make_refresh.clone());
        socket::start(socket::LEGACY_SOCKET_PATH, make_refresh);
    }

    fn rebuild_menu(&self) {
        let mtm = MainThreadMarker::new().unwrap();

        let show_sep = {
            let borrow = self.ivars().state.borrow();
            let Some(arc) = borrow.as_ref() else {
                return;
            };
            let guard = arc.lock().expect("state poisoned");
            guard.show_display_separator
        };

        let menu = NSMenu::new(mtm);

        let sep_title = if show_sep {
            ns_string!("Hide Display Separators")
        } else {
            ns_string!("Show Display Separators")
        };
        let sep_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                sep_title,
                Some(objc2::sel!(toggleSeparator:)),
                ns_string!(""),
            )
        };
        unsafe { sep_item.setTarget(Some(self)) };
        menu.addItem(&sep_item);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let refresh_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                ns_string!("Refresh"),
                Some(objc2::sel!(refresh)),
                ns_string!("r"),
            )
        };
        unsafe { refresh_item.setTarget(Some(self)) };
        menu.addItem(&refresh_item);

        let quit_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                ns_string!("Quit yabai-id"),
                Some(objc2::sel!(quit:)),
                ns_string!("q"),
            )
        };
        unsafe { quit_item.setTarget(Some(self)) };
        menu.addItem(&quit_item);

        if let Some(item) = self.ivars().status_item.borrow().as_ref() {
            item.setMenu(Some(&menu));
        }
    }
}
