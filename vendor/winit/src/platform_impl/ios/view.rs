#![allow(clippy::unnecessary_cast)]
use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{
    CGFloat, CGPoint, CGRect, MainThreadMarker, NSArray, NSNumber, NSObject, NSSet, NSString,
};
use objc2_ui_kit::{
    UICoordinateSpace, UIEvent, UIEventButtonMask, UIForceTouchCapability, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer, UIKey,
    UIKeyInput, UIKeyModifierFlags, UIPanGestureRecognizer, UIPress,
    UIPressesEvent, UIPinchGestureRecognizer, UIResponder,
    UIRotationGestureRecognizer, UIScrollTypeMask, UITapGestureRecognizer, UITextInputTraits,
    UITouch, UITouchPhase, UITouchType, UITraitEnvironment, UIView,
};

use super::app_state::{self, EventWrapper};
use super::window::WinitUIWindow;
use crate::dpi::PhysicalPosition;
use crate::event::{
    ElementState, Event, Force, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, Touch,
    TouchPhase, WindowEvent,
};
use crate::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKey, NativeKeyCode, PhysicalKey,
};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::{WindowAttributes, WindowId as RootWindowId};

pub struct WinitViewState {
    pinch_gesture_recognizer: RefCell<Option<Retained<UIPinchGestureRecognizer>>>,
    doubletap_gesture_recognizer: RefCell<Option<Retained<UITapGestureRecognizer>>>,
    rotation_gesture_recognizer: RefCell<Option<Retained<UIRotationGestureRecognizer>>>,
    pan_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,

    // for iOS delta references the start of the Gesture
    rotation_last_delta: Cell<CGFloat>,
    pinch_last_delta: Cell<CGFloat>,
    pan_last_delta: Cell<CGPoint>,

    // SolidCraft: a mouse or trackpad (UIKit's indirect pointer, with
    // `UIApplicationSupportsIndirectInputEvents`) as a mouse, not as touches: its buttons from the
    // event's button mask, the pointer over the view from a hover recognizer, and the wheel and
    // two-finger scroll from scroll-only pan recognizers.
    hover_gesture_recognizer: RefCell<Option<Retained<UIHoverGestureRecognizer>>>,
    wheel_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,
    trackpad_scroll_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,
    /// The buttons held, as `UIEventButtonMask` bits.
    pointer_buttons: Cell<isize>,
    /// The modifier keys last reported, from the pointer's events.
    pointer_modifiers: Cell<ModifiersState>,
    wheel_last_translation: Cell<CGPoint>,
    trackpad_scroll_last_translation: Cell<CGPoint>,
    /// SolidCraft: the hardware keys held whose press this view sent itself (by HID usage), so
    /// their release is sent too and not passed on to UIKit's text input.
    keys_sent: RefCell<Vec<i64>>,
    /// SolidCraft: whether the app is taking text (`set_ime_allowed`). The view stays the first
    /// responder either way, so keys keep coming; without text input the on-screen keyboard is
    /// hidden by an empty input view, and every key is sent as a key, not typed.
    text_input_wanted: Cell<bool>,
    no_keyboard_view: RefCell<Option<Retained<UIView>>>,
}

/// SolidCraft: winit's key for a hardware key's HID usage (USB HID usage page 7), with its named
/// key when it types no text. None for a key winit has no code for.
fn hid_key(usage: i64) -> Option<(KeyCode, Option<NamedKey>)> {
    use KeyCode as C;
    use NamedKey as N;
    const LETTERS: [KeyCode; 26] = [
        C::KeyA, C::KeyB, C::KeyC, C::KeyD, C::KeyE, C::KeyF, C::KeyG, C::KeyH, C::KeyI, C::KeyJ,
        C::KeyK, C::KeyL, C::KeyM, C::KeyN, C::KeyO, C::KeyP, C::KeyQ, C::KeyR, C::KeyS, C::KeyT,
        C::KeyU, C::KeyV, C::KeyW, C::KeyX, C::KeyY, C::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        C::Digit1, C::Digit2, C::Digit3, C::Digit4, C::Digit5, C::Digit6, C::Digit7, C::Digit8,
        C::Digit9, C::Digit0,
    ];
    const F_KEYS: [(KeyCode, NamedKey); 12] = [
        (C::F1, N::F1), (C::F2, N::F2), (C::F3, N::F3), (C::F4, N::F4), (C::F5, N::F5),
        (C::F6, N::F6), (C::F7, N::F7), (C::F8, N::F8), (C::F9, N::F9), (C::F10, N::F10),
        (C::F11, N::F11), (C::F12, N::F12),
    ];
    Some(match usage {
        0x04..=0x1D => (LETTERS[(usage - 0x04) as usize], None),
        0x1E..=0x27 => (DIGITS[(usage - 0x1E) as usize], None),
        0x28 => (C::Enter, Some(N::Enter)),
        0x29 => (C::Escape, Some(N::Escape)),
        0x2A => (C::Backspace, Some(N::Backspace)),
        0x2B => (C::Tab, Some(N::Tab)),
        0x2C => (C::Space, None),
        0x2D => (C::Minus, None),
        0x2E => (C::Equal, None),
        0x2F => (C::BracketLeft, None),
        0x30 => (C::BracketRight, None),
        0x31 => (C::Backslash, None),
        0x33 => (C::Semicolon, None),
        0x34 => (C::Quote, None),
        0x35 => (C::Backquote, None),
        0x36 => (C::Comma, None),
        0x37 => (C::Period, None),
        0x38 => (C::Slash, None),
        0x3A..=0x45 => {
            let (code, named) = F_KEYS[(usage - 0x3A) as usize];
            (code, Some(named))
        }
        0x49 => (C::Insert, Some(N::Insert)),
        0x4A => (C::Home, Some(N::Home)),
        0x4B => (C::PageUp, Some(N::PageUp)),
        0x4C => (C::Delete, Some(N::Delete)),
        0x4D => (C::End, Some(N::End)),
        0x4E => (C::PageDown, Some(N::PageDown)),
        0x4F => (C::ArrowRight, Some(N::ArrowRight)),
        0x50 => (C::ArrowLeft, Some(N::ArrowLeft)),
        0x51 => (C::ArrowDown, Some(N::ArrowDown)),
        0x52 => (C::ArrowUp, Some(N::ArrowUp)),
        0x58 => (C::NumpadEnter, Some(N::Enter)),
        0x64 => (C::IntlBackslash, None),
        _ => return None,
    })
}

/// SolidCraft: whether a modifier key (Control, Shift, Option, Command, left or right).
fn is_modifier_usage(usage: i64) -> bool {
    (0xE0..=0xE7).contains(&usage)
}

/// SolidCraft: points of a discrete (mouse wheel) scroll taken as one line, as a wheel's
/// `LineDelta`. Not measured on a device yet.
const WHEEL_POINTS_PER_LINE: f64 = 10.0;

/// SolidCraft: winit's button for a `UIEventButtonMask` bit (bit 0 is button 1, the primary).
fn mouse_button(bit: u32) -> MouseButton {
    match bit {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        n => MouseButton::Other(n as u16 + 1),
    }
}

/// SolidCraft: the modifier keys of UIKit's flags.
fn modifiers_state(flags: UIKeyModifierFlags) -> ModifiersState {
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::SHIFT, flags.contains(UIKeyModifierFlags::UIKeyModifierShift));
    state.set(ModifiersState::CONTROL, flags.contains(UIKeyModifierFlags::UIKeyModifierControl));
    state.set(ModifiersState::ALT, flags.contains(UIKeyModifierFlags::UIKeyModifierAlternate));
    state.set(ModifiersState::SUPER, flags.contains(UIKeyModifierFlags::UIKeyModifierCommand));
    state
}

declare_class!(
    pub(crate) struct WinitView;

    unsafe impl ClassType for WinitView {
        #[inherits(UIResponder, NSObject)]
        type Super = UIView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitUIView";
    }

    impl DeclaredClass for WinitView {
        type Ivars = WinitViewState;
    }

    unsafe impl WinitView {
        #[method(drawRect:)]
        fn draw_rect(&self, rect: CGRect) {
            let mtm = MainThreadMarker::new().unwrap();
            let window = self.window().unwrap();
            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::RedrawRequested,
                }),
            );
            let _: () = unsafe { msg_send![super(self), drawRect: rect] };
        }

        #[method(layoutSubviews)]
        fn layout_subviews(&self) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };

            let window = self.window().unwrap();
            let window_bounds = window.bounds();
            let screen = window.screen();
            let screen_space = screen.coordinateSpace();
            let screen_frame = self.convertRect_toCoordinateSpace(window_bounds, &screen_space);
            let scale_factor = screen.scale();
            let size = crate::dpi::LogicalSize {
                width: screen_frame.size.width as f64,
                height: screen_frame.size.height as f64,
            }
            .to_physical(scale_factor as f64);

            // If the app is started in landscape, the view frame and window bounds can be mismatched.
            // The view frame will be in portrait and the window bounds in landscape. So apply the
            // window bounds to the view frame to make it consistent.
            let view_frame = self.frame();
            if view_frame != window_bounds {
                self.setFrame(window_bounds);
            }

            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::Resized(size),
                }),
            );
        }

        #[method(setContentScaleFactor:)]
        fn set_content_scale_factor(&self, untrusted_scale_factor: CGFloat) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () =
                unsafe { msg_send![super(self), setContentScaleFactor: untrusted_scale_factor] };

            // `window` is null when `setContentScaleFactor` is invoked prior to `[UIWindow
            // makeKeyAndVisible]` at window creation time (either manually or internally by
            // UIKit when the `UIView` is first created), in which case we send no events here
            let window = match self.window() {
                Some(window) => window,
                None => return,
            };
            // `setContentScaleFactor` may be called with a value of 0, which means "reset the
            // content scale factor to a device-specific default value", so we can't use the
            // parameter here. We can query the actual factor using the getter
            let scale_factor = self.contentScaleFactor();
            assert!(
                !scale_factor.is_nan()
                    && scale_factor.is_finite()
                    && scale_factor.is_sign_positive()
                    && scale_factor > 0.0,
                "invalid scale_factor set on UIView",
            );
            let scale_factor = scale_factor as f64;
            let bounds = self.bounds();
            let screen = window.screen();
            let screen_space = screen.coordinateSpace();
            let screen_frame = self.convertRect_toCoordinateSpace(bounds, &screen_space);
            let size = crate::dpi::LogicalSize {
                width: screen_frame.size.width as f64,
                height: screen_frame.size.height as f64,
            };
            let window_id = RootWindowId(window.id());
            app_state::handle_nonuser_events(
                mtm,
                std::iter::once(EventWrapper::ScaleFactorChanged(
                    app_state::ScaleFactorChanged {
                        window,
                        scale_factor,
                        suggested_size: size.to_physical(scale_factor),
                    },
                ))
                .chain(std::iter::once(EventWrapper::StaticEvent(
                    Event::WindowEvent {
                        window_id,
                        event: WindowEvent::Resized(size.to_physical(scale_factor)),
                    },
                ))),
            );
        }

        #[method(touchesBegan:withEvent:)]
        fn touches_began(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            // SolidCraft: back as the first responder after the app was in the background.
            self.take_keys();
            self.handle_touches(touches, event)
        }

        #[method(touchesMoved:withEvent:)]
        fn touches_moved(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(touchesEnded:withEvent:)]
        fn touches_ended(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(touchesCancelled:withEvent:)]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(pinchGesture:)]
        fn pinch_gesture(&self, recognizer: &UIPinchGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pinch_last_delta.set(recognizer.scale());
                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(recognizer.scale());
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Ended => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(0.0);
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);
                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.scale())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: delta as f64,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(doubleTapGesture:)]
        fn double_tap_gesture(&self, recognizer: &UITapGestureRecognizer) {
            let window = self.window().unwrap();

            if recognizer.state() == UIGestureRecognizerState::Ended {
                let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::DoubleTapGesture {
                        device_id: DEVICE_ID,
                    },
                });

                let mtm = MainThreadMarker::new().unwrap();
                app_state::handle_nonuser_event(mtm, gesture_event);
            }
        }

        #[method(rotationGesture:)]
        fn rotation_gesture(&self, recognizer: &UIRotationGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().rotation_last_delta.set(0.0);

                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(recognizer.rotation());

                    (TouchPhase::Moved, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Ended => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(0.0);

                    (TouchPhase::Ended, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.rotation())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            // Make delta negative to match macos, convert to degrees
            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::RotationGesture {
                    device_id: DEVICE_ID,
                    delta: -delta.to_degrees() as _,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(panGesture:)]
        fn pan_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let window = self.window().unwrap();

            let translation = recognizer.translationInView(Some(self));

            let (phase, dx, dy) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pan_last_delta.set(translation);

                    (TouchPhase::Started, 0.0, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(translation);

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Moved, dx, dy)
                }
                UIGestureRecognizerState::Ended => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Ended, dx, dy)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -last_pan.x, -last_pan.y)
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };


            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PanGesture {
                    device_id: DEVICE_ID,
                    delta: PhysicalPosition::new(dx as _, dy as _),
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        // SolidCraft: the pointer moving over the view with no button down.
        #[method(hoverGesture:)]
        fn hover_gesture(&self, recognizer: &UIHoverGestureRecognizer) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let mut events = self.modifier_events(window_id, unsafe { recognizer.modifierFlags() });
            let position = self.physical(unsafe { recognizer.locationInView(None) });
            match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    events.push(Self::window_event(window_id, WindowEvent::CursorEntered { device_id: DEVICE_ID }));
                    events.push(Self::window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
                }
                UIGestureRecognizerState::Changed => {
                    events.push(Self::window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
                }
                // A button pressed ends the hover; the click's touch carries the pointer from
                // there, so the pointer stays over the view.
                UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled
                    if self.ivars().pointer_buttons.get() != 0 => {}
                UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    events.push(Self::window_event(window_id, WindowEvent::CursorLeft { device_id: DEVICE_ID }));
                }
                _ => {}
            }
            let mtm = MainThreadMarker::from(self);
            app_state::handle_nonuser_events(mtm, events);
        }

        // SolidCraft: a mouse wheel, as lines that never end (like a wheel elsewhere).
        #[method(wheelGesture:)]
        fn wheel_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let Some((dx, dy, _)) = self.scroll_delta(recognizer, &self.ivars().wheel_last_translation) else { return };
            let delta = MouseScrollDelta::LineDelta(
                (dx / WHEEL_POINTS_PER_LINE) as f32,
                (dy / WHEEL_POINTS_PER_LINE) as f32,
            );
            self.send_scroll(recognizer, delta, TouchPhase::Moved);
        }

        // SolidCraft: two fingers on a trackpad, as pixels in a bracket (Started … Ended).
        #[method(trackpadScrollGesture:)]
        fn trackpad_scroll_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let Some((dx, dy, phase)) = self.scroll_delta(recognizer, &self.ivars().trackpad_scroll_last_translation) else { return };
            let scale = self.contentScaleFactor() as f64;
            let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(dx * scale, dy * scale));
            self.send_scroll(recognizer, delta, phase);
        }

        // SolidCraft: a hardware keyboard. A key that types no text (Enter, Escape, Tab, the
        // arrows, Home, End, Page Up and Down, forward Delete, F1-F12) or is pressed with Command
        // or Control is sent as a key here; any other key goes on to UIKit's text input, which
        // composes it (Korean, Japanese) and types it through `insertText`, as before.
        #[method(pressesBegan:withEvent:)]
        fn presses_began(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Pressed) {
                let _: () = unsafe { msg_send![super(self), pressesBegan: presses, withEvent: event] };
            }
        }

        #[method(pressesEnded:withEvent:)]
        fn presses_ended(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesEnded: presses, withEvent: event] };
            }
        }

        #[method(pressesCancelled:withEvent:)]
        fn presses_cancelled(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesCancelled: presses, withEvent: event] };
            }
        }

        // SolidCraft: the keys come to this view from the start.
        #[method(didMoveToWindow)]
        fn did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), didMoveToWindow] };
            self.take_keys();
        }

        // SolidCraft: no on-screen keyboard unless the app takes text (an empty view in its
        // place); the system's keyboard (nil) when it does.
        #[method_id(inputView)]
        fn input_view(&self) -> Option<Retained<UIView>> {
            // (No early `return` here: `method_id` wraps the body's value.)
            if self.ivars().text_input_wanted.get() {
                None
            } else {
                Some(self.no_keyboard_view())
            }
        }

        #[method(canBecomeFirstResponder)]
        fn can_become_first_responder(&self) -> bool {
            true
        }
    }

    unsafe impl NSObjectProtocol for WinitView {}

    unsafe impl UIGestureRecognizerDelegate for WinitView {
        #[method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:)]
        fn should_recognize_simultaneously(&self, _gesture_recognizer: &UIGestureRecognizer, _other_gesture_recognizer: &UIGestureRecognizer) -> bool {
            true
        }
    }

    unsafe impl UITextInputTraits for WinitView {
    }

    unsafe impl UIKeyInput for WinitView {
        #[method(hasText)]
        fn has_text(&self) -> bool {
            true
        }

        #[method(insertText:)]
        fn insert_text(&self, text: &NSString) {
            self.handle_insert_text(text)
        }

        #[method(deleteBackward)]
        fn delete_backward(&self) {
            self.handle_delete_backward()
        }
    }
);

impl WinitView {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        window_attributes: &WindowAttributes,
        frame: CGRect,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(WinitViewState {
            pinch_gesture_recognizer: RefCell::new(None),
            doubletap_gesture_recognizer: RefCell::new(None),
            rotation_gesture_recognizer: RefCell::new(None),
            pan_gesture_recognizer: RefCell::new(None),

            rotation_last_delta: Cell::new(0.0),
            pinch_last_delta: Cell::new(0.0),
            pan_last_delta: Cell::new(CGPoint { x: 0.0, y: 0.0 }),

            hover_gesture_recognizer: RefCell::new(None),
            wheel_gesture_recognizer: RefCell::new(None),
            trackpad_scroll_gesture_recognizer: RefCell::new(None),
            pointer_buttons: Cell::new(0),
            pointer_modifiers: Cell::new(ModifiersState::empty()),
            wheel_last_translation: Cell::new(CGPoint { x: 0.0, y: 0.0 }),
            trackpad_scroll_last_translation: Cell::new(CGPoint { x: 0.0, y: 0.0 }),
            keys_sent: RefCell::new(Vec::new()),
            text_input_wanted: Cell::new(false),
            no_keyboard_view: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), initWithFrame: frame] };

        this.setMultipleTouchEnabled(true);
        this.recognize_pointer();

        if let Some(scale_factor) = window_attributes.platform_specific.scale_factor {
            this.setContentScaleFactor(scale_factor as _);
        }

        this
    }

    fn window(&self) -> Option<Retained<WinitUIWindow>> {
        // SAFETY: `WinitView`s are always installed in a `WinitUIWindow`
        (**self).window().map(|window| unsafe { Retained::cast(window) })
    }

    pub(crate) fn recognize_pinch_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pinch_gesture_recognizer.borrow().is_none() {
                let pinch = unsafe {
                    UIPinchGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(pinchGesture:)),
                    )
                };
                pinch.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&pinch);
                self.ivars().pinch_gesture_recognizer.replace(Some(pinch));
            }
        } else if let Some(recognizer) = self.ivars().pinch_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_pan_gesture(
        &self,
        should_recognize: bool,
        minimum_number_of_touches: u8,
        maximum_number_of_touches: u8,
    ) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pan_gesture_recognizer.borrow().is_none() {
                let pan = unsafe {
                    UIPanGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(panGesture:)),
                    )
                };
                pan.setDelegate(Some(ProtocolObject::from_ref(self)));
                pan.setMinimumNumberOfTouches(minimum_number_of_touches as _);
                pan.setMaximumNumberOfTouches(maximum_number_of_touches as _);
                self.addGestureRecognizer(&pan);
                self.ivars().pan_gesture_recognizer.replace(Some(pan));
            }
        } else if let Some(recognizer) = self.ivars().pan_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_doubletap_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().doubletap_gesture_recognizer.borrow().is_none() {
                let tap = unsafe {
                    UITapGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(doubleTapGesture:)),
                    )
                };
                tap.setDelegate(Some(ProtocolObject::from_ref(self)));
                tap.setNumberOfTapsRequired(2);
                tap.setNumberOfTouchesRequired(1);
                self.addGestureRecognizer(&tap);
                self.ivars().doubletap_gesture_recognizer.replace(Some(tap));
            }
        } else if let Some(recognizer) = self.ivars().doubletap_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_rotation_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().rotation_gesture_recognizer.borrow().is_none() {
                let rotation = unsafe {
                    UIRotationGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(rotationGesture:)),
                    )
                };
                rotation.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&rotation);
                self.ivars().rotation_gesture_recognizer.replace(Some(rotation));
            }
        } else if let Some(recognizer) = self.ivars().rotation_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    /// SolidCraft: the hover and the two scroll recognizers. None of them takes touches, so
    /// fingers and the pencil reach `touchesBegan` as before.
    fn recognize_pointer(&self) {
        let mtm = MainThreadMarker::from(self);
        let no_touches: Retained<NSArray<NSNumber>> = NSArray::new();

        let hover = unsafe {
            UIHoverGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(hoverGesture:)),
            )
        };
        hover.setDelegate(Some(ProtocolObject::from_ref(self)));
        unsafe { hover.setCancelsTouchesInView(false) };
        self.addGestureRecognizer(&hover);
        self.ivars().hover_gesture_recognizer.replace(Some(hover));

        for (action, mask, slot) in [
            (
                sel!(wheelGesture:),
                UIScrollTypeMask::Discrete,
                &self.ivars().wheel_gesture_recognizer,
            ),
            (
                sel!(trackpadScrollGesture:),
                UIScrollTypeMask::Continuous,
                &self.ivars().trackpad_scroll_gesture_recognizer,
            ),
        ] {
            let scroll = unsafe {
                UIPanGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self), Some(action))
            };
            scroll.setDelegate(Some(ProtocolObject::from_ref(self)));
            unsafe {
                scroll.setAllowedScrollTypesMask(mask);
                // Scroll only: a finger drag stays a touch.
                scroll.setAllowedTouchTypes(&no_touches);
                scroll.setCancelsTouchesInView(false);
            }
            self.addGestureRecognizer(&scroll);
            slot.replace(Some(scroll));
        }
    }

    /// SolidCraft: a point in the window's coordinates, in physical pixels.
    fn physical(&self, logical: CGPoint) -> PhysicalPosition<f64> {
        PhysicalPosition::from_logical::<(f64, f64), f64>(
            (logical.x as _, logical.y as _),
            self.contentScaleFactor() as f64,
        )
    }

    fn window_event(window_id: RootWindowId, event: WindowEvent) -> EventWrapper {
        EventWrapper::StaticEvent(Event::WindowEvent { window_id, event })
    }

    /// SolidCraft: `ModifiersChanged` when the keys held differ from the last ones reported.
    fn modifier_events(&self, window_id: RootWindowId, flags: UIKeyModifierFlags) -> Vec<EventWrapper> {
        let state = modifiers_state(flags);
        if self.ivars().pointer_modifiers.replace(state) == state {
            return Vec::new();
        }
        vec![Self::window_event(window_id, WindowEvent::ModifiersChanged(Modifiers::from(state)))]
    }

    /// SolidCraft: the scroll since the last call in points, and its phase; None for a state
    /// with nothing to send.
    fn scroll_delta(
        &self,
        recognizer: &UIPanGestureRecognizer,
        last: &Cell<CGPoint>,
    ) -> Option<(f64, f64, TouchPhase)> {
        let translation = recognizer.translationInView(Some(self));
        let zero = CGPoint { x: 0.0, y: 0.0 };
        let (phase, previous) = match recognizer.state() {
            UIGestureRecognizerState::Began => (TouchPhase::Started, last.replace(translation)),
            UIGestureRecognizerState::Changed => (TouchPhase::Moved, last.replace(translation)),
            UIGestureRecognizerState::Ended => (TouchPhase::Ended, last.replace(zero)),
            UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                last.set(zero);
                return Some((0.0, 0.0, TouchPhase::Cancelled));
            }
            _ => return None,
        };
        let previous = if phase == TouchPhase::Started { zero } else { previous };
        Some(((translation.x - previous.x) as f64, (translation.y - previous.y) as f64, phase))
    }

    /// SolidCraft: the pointer's place and keys, then the scroll.
    fn send_scroll(&self, recognizer: &UIPanGestureRecognizer, delta: MouseScrollDelta, phase: TouchPhase) {
        let Some(window) = self.window() else { return };
        let window_id = RootWindowId(window.id());
        let mut events = self.modifier_events(window_id, unsafe { recognizer.modifierFlags() });
        let position = self.physical(unsafe { recognizer.locationInView(None) });
        events.push(Self::window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
        if delta != MouseScrollDelta::LineDelta(0.0, 0.0) || phase != TouchPhase::Moved {
            events.push(Self::window_event(window_id, WindowEvent::MouseWheel { device_id: DEVICE_ID, delta, phase }));
        }
        let mtm = MainThreadMarker::from(self);
        app_state::handle_nonuser_events(mtm, events);
    }

    /// SolidCraft: a mouse or trackpad click and drag: the pointer's place, then a press or a
    /// release for each button whose bit in the event's mask changed. All buttons are up once
    /// the touch ends; a touch that begins with an empty mask is a primary click.
    fn pointer_events(
        &self,
        window_id: RootWindowId,
        touch: &UITouch,
        event: Option<&UIEvent>,
    ) -> Vec<EventWrapper> {
        let phase = touch.phase();
        let mut events = match event {
            Some(e) => self.modifier_events(window_id, unsafe { e.modifierFlags() }),
            None => Vec::new(),
        };
        let position = self.physical(touch.locationInView(None));
        events.push(Self::window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
        let held = match phase {
            UITouchPhase::Ended | UITouchPhase::Cancelled => 0,
            _ => {
                let mask = event.map_or(UIEventButtonMask::Primary, |e| unsafe { e.buttonMask() });
                if mask.0 == 0 { UIEventButtonMask::Primary.0 } else { mask.0 }
            }
        };
        let before = self.ivars().pointer_buttons.replace(held);
        let changed = before ^ held;
        for bit in 0..(isize::BITS - 1) {
            if changed & (1 << bit) == 0 {
                continue;
            }
            let state = if held & (1 << bit) != 0 { ElementState::Pressed } else { ElementState::Released };
            events.push(Self::window_event(
                window_id,
                WindowEvent::MouseInput { device_id: DEVICE_ID, state, button: mouse_button(bit) },
            ));
        }
        events
    }

    fn handle_touches(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
        let window = self.window().unwrap();
        let mut touch_events = Vec::new();
        let os_supports_force = app_state::os_capabilities().force_touch;
        for touch in touches {
            let logical_location = touch.locationInView(None);
            let touch_type = touch.r#type();
            // SolidCraft: a mouse or trackpad is a mouse.
            if touch_type == UITouchType::IndirectPointer {
                touch_events.extend(self.pointer_events(RootWindowId(window.id()), touch, event));
                continue;
            }
            let force = if os_supports_force {
                let trait_collection = self.traitCollection();
                let touch_capability = trait_collection.forceTouchCapability();
                // Both the OS _and_ the device need to be checked for force touch support.
                if touch_capability == UIForceTouchCapability::Available
                    || touch_type == UITouchType::Pencil
                {
                    let force = touch.force();
                    let max_possible_force = touch.maximumPossibleForce();
                    let altitude_angle: Option<f64> = if touch_type == UITouchType::Pencil {
                        let angle = touch.altitudeAngle();
                        Some(angle as _)
                    } else {
                        None
                    };
                    Some(Force::Calibrated {
                        force: force as _,
                        max_possible_force: max_possible_force as _,
                        altitude_angle,
                    })
                } else {
                    None
                }
            } else {
                None
            };
            let touch_id = touch as *const UITouch as u64;
            let phase = touch.phase();
            let phase = match phase {
                UITouchPhase::Began => TouchPhase::Started,
                UITouchPhase::Moved => TouchPhase::Moved,
                // 2 is UITouchPhase::Stationary and is not expected here
                UITouchPhase::Ended => TouchPhase::Ended,
                UITouchPhase::Cancelled => TouchPhase::Cancelled,
                _ => panic!("unexpected touch phase: {phase:?}"),
            };

            let physical_location = {
                let scale_factor = self.contentScaleFactor();
                PhysicalPosition::from_logical::<(f64, f64), f64>(
                    (logical_location.x as _, logical_location.y as _),
                    scale_factor as f64,
                )
            };
            touch_events.push(EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::Touch(Touch {
                    device_id: DEVICE_ID,
                    id: touch_id,
                    location: physical_location,
                    force,
                    phase,
                }),
            }));
        }
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_events(mtm, touch_events);
    }

    /// SolidCraft: the empty input view that keeps the on-screen keyboard hidden.
    fn no_keyboard_view(&self) -> Retained<UIView> {
        let mtm = MainThreadMarker::from(self);
        let mut slot = self.ivars().no_keyboard_view.borrow_mut();
        let view = slot.get_or_insert_with(|| {
            let zero = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: objc2_foundation::CGSize { width: 0.0, height: 0.0 },
            };
            UIView::initWithFrame(mtm.alloc(), zero)
        });
        view.clone()
    }

    /// SolidCraft: be the first responder, so the keyboard's keys come here.
    fn take_keys(&self) {
        if self.window().is_some() && !unsafe { self.isFirstResponder() } {
            unsafe { self.becomeFirstResponder() };
        }
    }

    /// SolidCraft: `Window::set_ime_allowed`: show or hide the on-screen keyboard and choose
    /// whether keys type text, staying the first responder.
    pub(crate) fn set_text_input(&self, wanted: bool) {
        self.ivars().text_input_wanted.set(wanted);
        self.take_keys();
        unsafe { self.reloadInputViews() };
    }

    /// SolidCraft: send the keys of `presses` that this view takes (see `pressesBegan`), and
    /// the keys held as `ModifiersChanged`. True when every press was taken, so none goes on to
    /// UIKit (a press with no key, before iPadOS 13.4, goes on).
    fn handle_presses(&self, presses: &NSSet<UIPress>, state: ElementState) -> bool {
        let Some(window) = self.window() else { return false };
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::from(self);
        let mut events = Vec::new();
        let mut all_taken = true;
        for press in presses {
            let Some(key) = (unsafe { press.key(mtm) }) else {
                all_taken = false;
                continue;
            };
            let flags = unsafe { key.modifierFlags() };
            events.extend(self.modifier_events(window_id, flags));
            // The HID usage (CFIndex).
            let usage = unsafe { key.keyCode() }.0 as i64;
            match self.key_event(&key, usage, flags, state) {
                Some(event) => events.push(Self::window_event(window_id, event)),
                None => all_taken = false,
            }
        }
        if !events.is_empty() {
            app_state::handle_nonuser_events(mtm, events);
        }
        all_taken
    }

    /// SolidCraft: the key event for one hardware key, when this view takes it: on a press,
    /// any key while the app takes no text, else a key that types no text or one held with
    /// Command or Control; on a release, a key whose press it took. Modifier keys only change
    /// the modifiers.
    fn key_event(
        &self,
        key: &UIKey,
        usage: i64,
        flags: UIKeyModifierFlags,
        state: ElementState,
    ) -> Option<WindowEvent> {
        if is_modifier_usage(usage) {
            return None;
        }
        let (code, named) = hid_key(usage)?;
        let sent = &self.ivars().keys_sent;
        match state {
            ElementState::Pressed => {
                let shortcut = flags.intersects(
                    UIKeyModifierFlags::UIKeyModifierCommand | UIKeyModifierFlags::UIKeyModifierControl,
                );
                // While the app takes text, typing keys go to the text input, which composes
                // them (Korean); Backspace too, which deletes inside a composition. Otherwise
                // every key is a key, so single-key shortcuts work whatever the input language
                // (a Korean ㅣ is still the L key).
                if self.ivars().text_input_wanted.get() {
                    if named == Some(NamedKey::Backspace) && !shortcut {
                        return None;
                    }
                    if named.is_none() && !shortcut {
                        return None;
                    }
                }
                let mut held = sent.borrow_mut();
                if !held.contains(&usage) {
                    held.push(usage);
                }
            }
            ElementState::Released => {
                let mut held = sent.borrow_mut();
                let at = held.iter().position(|u| *u == usage)?;
                held.swap_remove(at);
            }
        }
        let logical_key = match named {
            Some(named) => Key::Named(named),
            None => {
                let typed = unsafe { key.charactersIgnoringModifiers() }.to_string().to_lowercase();
                if typed.is_empty() {
                    Key::Unidentified(NativeKey::Unidentified)
                } else {
                    Key::Character(smol_str::SmolStr::new(typed))
                }
            }
        };
        let location = if code == KeyCode::NumpadEnter { KeyLocation::Numpad } else { KeyLocation::Standard };
        Some(WindowEvent::KeyboardInput {
            device_id: DEVICE_ID,
            event: KeyEvent {
                physical_key: PhysicalKey::Code(code),
                logical_key,
                text: None,
                location,
                state,
                repeat: false,
                platform_specific: KeyEventExtra {},
            },
            is_synthetic: false,
        })
    }

    /// SolidCraft: Return on the on-screen keyboard types a newline; send it as the Enter key, as
    /// a hardware keyboard's is.
    fn send_enter(&self, window_id: RootWindowId) {
        let mtm = MainThreadMarker::from(self);
        app_state::handle_nonuser_events(
            mtm,
            [ElementState::Pressed, ElementState::Released].map(|state| {
                Self::window_event(
                    window_id,
                    WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event: KeyEvent {
                            physical_key: PhysicalKey::Code(KeyCode::Enter),
                            logical_key: Key::Named(NamedKey::Enter),
                            text: None,
                            location: KeyLocation::Standard,
                            state,
                            repeat: false,
                            platform_specific: KeyEventExtra {},
                        },
                        is_synthetic: false,
                    },
                )
            }),
        );
    }

    fn handle_insert_text(&self, text: &NSString) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let typed = text.to_string();
        if typed == "\n" || typed == "\r" {
            self.send_enter(window_id);
            return;
        }
        let mtm = MainThreadMarker::new().unwrap();
        // send individual events for each character
        app_state::handle_nonuser_events(
            mtm,
            text.to_string().chars().flat_map(|c| {
                let text = smol_str::SmolStr::from_iter([c]);
                // Emit both press and release events
                [ElementState::Pressed, ElementState::Released].map(|state| {
                    EventWrapper::StaticEvent(Event::WindowEvent {
                        window_id,
                        event: WindowEvent::KeyboardInput {
                            event: KeyEvent {
                                text: if state == ElementState::Pressed {
                                    Some(text.clone())
                                } else {
                                    None
                                },
                                state,
                                location: KeyLocation::Standard,
                                repeat: false,
                                logical_key: Key::Character(text.clone()),
                                physical_key: PhysicalKey::Unidentified(
                                    NativeKeyCode::Unidentified,
                                ),
                                platform_specific: KeyEventExtra {},
                            },
                            is_synthetic: false,
                            device_id: DEVICE_ID,
                        },
                    })
                })
            }),
        );
    }

    fn handle_delete_backward(&self) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_events(
            mtm,
            [ElementState::Pressed, ElementState::Released].map(|state| {
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id,
                    event: WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event: KeyEvent {
                            state,
                            logical_key: Key::Named(NamedKey::Backspace),
                            physical_key: PhysicalKey::Code(KeyCode::Backspace),
                            platform_specific: KeyEventExtra {},
                            repeat: false,
                            location: KeyLocation::Standard,
                            text: None,
                        },
                        is_synthetic: false,
                    },
                })
            }),
        );
    }
}
