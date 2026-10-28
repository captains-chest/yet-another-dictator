import Foundation
import Synchronization
import Testing
@testable import ModelSession

final class Gate: @unchecked Sendable {
    private let lock = NSLock()
    private var openFlag = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func wait() async {
        await withCheckedContinuation { continuation in
            let immediate = lock.withLock { () -> Bool in
                if openFlag { return true }
                waiters.append(continuation)
                return false
            }
            if immediate { continuation.resume() }
        }
    }

    func open() {
        let pending = lock.withLock {
            openFlag = true
            defer { waiters.removeAll() }
            return waiters
        }
        pending.forEach { $0.resume() }
    }
}

final class EventLog: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [String] = []
    func add(_ event: String) { lock.withLock { events.append(event) } }
    var all: [String] { lock.withLock { events } }
    func count(_ event: String) -> Int { lock.withLock { events.filter { $0 == event }.count } }
}

struct FakeModel: LoadedSpeechModel {
    let log: EventLog
    let hold: Gate?
    let failFirst: Bool

    func transcribe(_ samples: [Float], language: String?, stop: StopSignal) async throws -> String {
        log.add("start\(samples.first.map { Int($0) } ?? 0)")
        if let hold { await hold.wait() }
        // As in the adapters: a stop set while running cancels the transcription.
        if failFirst { throw CancellationError() }
        if stop.isSet { log.add("end\(samples.first.map { Int($0) } ?? 0)"); throw CancellationError() }
        log.add("end\(samples.first.map { Int($0) } ?? 0)")
        return "fake \(stop.isSet)"
    }
}

@Suite struct ModelSessionTests {
    let root = FileManager.default.temporaryDirectory.appending(path: "ModelSessionTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    let log = EventLog()

    func makeSession(
        gate: Gate? = nil, failFirstLoad: Bool = false, modelHold: Gate? = nil
    ) throws -> ModelSession {
        try FileManager.default.createDirectory(
            at: root.appending(path: "model"), withIntermediateDirectories: true
        )
        let file = root.appending(path: "model/needed.txt")
        try Data().write(to: file)
        let counter = Mutex(0)
        let log = log
        return ModelSession(
            modelFolder: root.appending(path: "model"), requiredFiles: ["needed.txt"]
        ) { _ in
            let log2 = log
            let attempt = counter.withLock { value in
                value += 1
                return value
            }
            log2.add("load")
            if failFirstLoad, attempt == 1 { throw URLError(.badServerResponse) }
            if let gate { await gate.wait() }
            return FakeModel(log: log2, hold: modelHold, failFirst: false)
        }
    }

    func waitFor(_ event: String) async {
        for _ in 0..<500 where log.count(event) == 0 {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    @Test func missingFilesThrowsAndNeverLoads() async throws {
        let empty = root.appending(path: "empty", directoryHint: .isDirectory)
        let session2 = ModelSession(modelFolder: empty, requiredFiles: ["needed.txt"]) { _ in
            self.log.add("load2")
            return FakeModel(log: self.log, hold: nil, failFirst: false)
        }
        await #expect(throws: ModelSession.Failure.self) { try await session2.prepare() }
        #expect(log.count("load2") == 0)
    }

    @Test func concurrentPrepareSharesOneLoad() async throws {
        let gate = Gate()
        let session = try makeSession(gate: gate)
        let one = Task { try await session.prepare() }
        let two = Task { try await session.prepare() }
        await waitFor("load")
        gate.open()
        try await one.value
        try await two.value
        #expect(log.count("load") == 1)
    }

    @Test func failedLoadThrowsToWaitersThenRetries() async throws {
        let session = try makeSession(failFirstLoad: true)
        let one = Task { try await session.prepare() }
        let two = Task { try await session.prepare() }
        await #expect(throws: URLError.self) { try await one.value }
        await #expect(throws: URLError.self) { try await two.value }
        try await session.prepare()
        #expect(log.count("load") == 2)
    }

    @Test func transcribeWithoutPrepareLoadsFirst() async throws {
        let session = try makeSession()
        #expect(try await session.transcribe([42], language: nil) == "fake false")
        #expect(log.count("load") == 1)
    }

    @Test func cancelledHeldTranscriptionAndNextSequenced() async throws {
        let hold = Gate()
        let session = try makeSession(modelHold: hold)
        let first = Task { try await session.transcribe([1], language: nil) }
        await waitFor("start1")
        let second = Task { try await session.transcribe([2], language: nil) }
        first.cancel()
        hold.open()
        await #expect(throws: CancellationError.self) { try await first.value }
        #expect(try await second.value == "fake false")
        #expect(log.all.filter { $0.hasPrefix("start") || $0.hasPrefix("end") } == ["start1", "end1", "start2", "end2"])
    }

    @Test func queuedCancellationNeverReachesModel() async throws {
        let hold = Gate()
        let session = try makeSession(modelHold: hold)
        let first = Task { try await session.transcribe([1], language: nil) }
        await waitFor("start1")
        let second = Task { try await session.transcribe([2], language: nil) }
        try? await Task.sleep(for: .milliseconds(20))
        second.cancel()
        hold.open()
        #expect(try await first.value == "fake false")
        await #expect(throws: CancellationError.self) { try await second.value }
        #expect(log.count("start2") == 0)
    }

    @Test func completedTranscriptionNeverSeesStop() async throws {
        let session = try makeSession()
        #expect(try await session.transcribe([3], language: nil) == "fake false")
    }
}
