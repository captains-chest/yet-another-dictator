import CoreGraphics
import DictationCore
import Foundation

/// System-wide shortcut via a CoreGraphics event tap, so any key or chord can be used:
/// modifier-only (e.g. Right ⌥), fn, Caps Lock, bare keys, several keys at once.
///
/// Needs Input Monitoring permission. Events are only matched against the shortcut in memory;
/// nothing is stored or logged. The shortcut's own non-modifier keys are withheld from other apps
/// when macOS allows an active tap; otherwise the tap is listen-only and they pass through.
///
/// The tap runs on its own high-priority thread, not the main thread. With a tap that can withhold
/// keys, every keystroke on the Mac waits for our callback, so a busy main thread (starting the
/// microphone, typing a long dictation, SwiftUI work) would otherwise stall all typing system-wide
/// and get the tap disabled by macOS. Matching happens on that thread; only the resulting
/// press/release/interrupt actions hop to the main actor.
@MainActor
final class EventTapHotkey {
    enum Failure: Error, CustomStringConvertible {
        case inputMonitoringDenied
        case tapUnavailable
        var description: String {
            switch self {
            case .inputMonitoringDenied:
                "Grant Input Monitoring so Dictator can detect the shortcut, then reopen this window."
            case .tapUnavailable:
                "macOS refused to watch the keyboard. Check Input Monitoring, then relaunch Dictator."
            }
        }
    }

    /// How the tap was installed. macOS may refuse a tap that can withhold events (e.g. for a
    /// sandboxed app); the listen-only fallback still detects the shortcut, but its keys then also
    /// reach the focused app.
    enum Mode: Equatable, Sendable {
        case withholdsKeys
        case listenOnly
    }

    var onPress: (() -> Void)?
    var onRelease: (() -> Void)?
    var onInterrupt: (() -> Void)?
    /// Called when the optional undo shortcut is pressed.
    var onUndo: (() -> Void)?
    /// Called when the user types a key that isn't part of the shortcut, or clicks outside the menu
    /// bar, so the text cursor may have moved. Carries no key or position information.
    var onOtherInput: (() -> Void)?

    private(set) var isRegistered = false
    /// The mode of the current tap, or of the last one when unregistered; nil until first registered.
    private(set) var mode: Mode?
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private let context: TapContext
    private let thread = TapThread()

    init() {
        context = TapContext(ownPID: Int64(ProcessInfo.processInfo.processIdentifier))
        context.deliver = { [weak self] event in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.dispatch(event) }
            }
        }
        thread.start()
    }

    func register(_ shortcut: Shortcut, undo: Shortcut? = nil) throws {
        unregister()
        guard CGPreflightListenEventAccess() else {
            CGRequestListenEventAccess()
            throw Failure.inputMonitoringDenied
        }
        let types: [CGEventType] = [.keyDown, .keyUp, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown]
        let mask = types.reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        let refcon = Unmanaged.passUnretained(context).toOpaque()
        let callback: CGEventTapCallBack = { _, type, event, refcon in
            guard let refcon else { return Unmanaged.passUnretained(event) }
            let context = Unmanaged<TapContext>.fromOpaque(refcon).takeUnretainedValue()
            // Runs on the tap thread.
            return context.handle(type, event) ? nil : Unmanaged.passUnretained(event)
        }
        func create(_ options: CGEventTapOptions) -> CFMachPort? {
            CGEvent.tapCreate(
                tap: .cgSessionEventTap, place: .headInsertEventTap, options: options,
                eventsOfInterest: mask, callback: callback, userInfo: refcon
            )
        }
        let tap: CFMachPort
        if let active = create(.defaultTap) {
            tap = active
            mode = .withholdsKeys
        } else if let passive = create(.listenOnly) {
            tap = passive
            mode = .listenOnly
        } else {
            throw Failure.tapUnavailable
        }
        context.install(shortcut: shortcut, undo: undo, tap: tap)
        let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
        let runLoop = thread.waitForRunLoop()
        CFRunLoopAddSource(runLoop, source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        CFRunLoopWakeUp(runLoop)
        self.tap = tap
        self.source = source
        isRegistered = true
    }

    func unregister() {
        if let tap {
            CGEvent.tapEnable(tap: tap, enable: false)
            CFMachPortInvalidate(tap)
        }
        if let source { CFRunLoopRemoveSource(thread.waitForRunLoop(), source, .commonModes) }
        context.install(shortcut: nil, undo: nil, tap: nil)
        tap = nil
        source = nil
        isRegistered = false
    }

    private func dispatch(_ event: TapContext.Event) {
        switch event {
        case .action(.pressed): onPress?()
        case .action(.released): onRelease?()
        case .action(.interrupted): onInterrupt?()
        case .otherInput: onOtherInput?()
        case .undo: onUndo?()
        }
    }
}

/// State used on the tap thread. Guarded by a lock because `install` is called from the main thread.
private final class TapContext: @unchecked Sendable {
    enum Event: Sendable {
        case action(ChordMatcher.Action)
        case otherInput
        case undo
    }

    var deliver: @Sendable (Event) -> Void = { _ in }
    private let ownPID: Int64
    private let lock = NSLock()
    private var shortcut: Shortcut?
    private var matcher: ChordMatcher?
    private var undoShortcut: Shortcut?
    private var undoMatcher: ChordMatcher?
    private var tap: CFMachPort?

    init(ownPID: Int64) { self.ownPID = ownPID }

    func install(shortcut: Shortcut?, undo: Shortcut?, tap: CFMachPort?) {
        lock.withLock {
            self.shortcut = shortcut
            self.matcher = shortcut.map(ChordMatcher.init)
            self.undoShortcut = undo
            self.undoMatcher = undo.map(ChordMatcher.init)
            self.tap = tap
        }
    }

    /// Returns true to withhold the event from other apps.
    func handle(_ type: CGEventType, _ event: CGEvent) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        switch type {
        case .tapDisabledByTimeout, .tapDisabledByUserInput:
            // Keys may have been released while disabled; start tracking afresh.
            if let tap, let shortcut {
                matcher = ChordMatcher(shortcut)
                undoMatcher = undoShortcut.map(ChordMatcher.init)
                CGEvent.tapEnable(tap: tap, enable: true)
            }
            return false
        default:
            break
        }
        // Ignore the text we type ourselves.
        guard event.getIntegerValueField(.eventSourceUnixProcessID) != ownPID else { return false }
        switch type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            // Clicks in the menu bar (including our own menu) or in our windows don't move the
            // text cursor; any other click may.
            if event.getIntegerValueField(.eventTargetUnixProcessID) != ownPID, !Self.isInMenuBar(event.location) {
                deliver(.otherInput)
            }
            return false
        default:
            break
        }
        let code = UInt16(truncatingIfNeeded: event.getIntegerValueField(.keyboardEventKeycode))
        let keyEvent: KeyEvent
        switch type {
        case .keyDown: keyEvent = .down(code)
        case .keyUp: keyEvent = .up(code)
        case .flagsChanged: keyEvent = .flagsChanged(code, flags: event.flags.rawValue)
        default: return false
        }
        guard var matcher else { return false }
        let (actions, swallow) = matcher.handle(keyEvent)
        self.matcher = matcher
        for action in actions { deliver(.action(action)) }

        var undoActions: [ChordMatcher.Action] = []
        var undoSwallow = false
        if var undoMatcher {
            (undoActions, undoSwallow) = undoMatcher.handle(keyEvent)
            self.undoMatcher = undoMatcher
            if undoActions.contains(.pressed) { deliver(.undo) }
        }

        if case .down = keyEvent, !swallow, !undoSwallow, actions.isEmpty, undoActions.isEmpty {
            deliver(.otherInput)
        }
        return swallow || undoSwallow
    }

    /// Whether a point (global, top-left origin) is in the menu bar strip of the display it's on.
    private static func isInMenuBar(_ point: CGPoint) -> Bool {
        var display: CGDirectDisplayID = 0
        var count: UInt32 = 0
        guard CGGetDisplaysWithPoint(point, 1, &display, &count) == .success, count > 0 else { return false }
        // Tall enough for menu bars next to a camera notch.
        return point.y - CGDisplayBounds(display).minY < 44
    }
}

/// A dedicated thread with its own run loop for the event tap.
private final class TapThread: Thread {
    private let ready = DispatchSemaphore(value: 0)
    private var runLoop: CFRunLoop?

    override init() {
        super.init()
        name = "Dictator event tap"
        qualityOfService = .userInteractive
    }

    override func main() {
        runLoop = CFRunLoopGetCurrent()
        // A port keeps the run loop alive while no tap is installed.
        RunLoop.current.add(NSMachPort(), forMode: .default)
        ready.signal()
        while true { RunLoop.current.run(mode: .default, before: .distantFuture) }
    }

    /// The thread's run loop, waiting briefly for the thread to come up the first time.
    func waitForRunLoop() -> CFRunLoop {
        if let runLoop { return runLoop }
        ready.wait()
        ready.signal()
        return runLoop!
    }
}
