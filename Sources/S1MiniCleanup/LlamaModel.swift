import Foundation
import llama
import TranscriptCleanup

/// One loaded S1-mini GGUF in llama.cpp, with one context. Not thread-safe: `S1MiniCleaner` runs
/// one cleanup at a time. Weights and work stay on the GPU (Metal) on Apple Silicon.
final class LlamaModel: @unchecked Sendable {
    enum Failure: Error, CustomStringConvertible {
        case loadFailed
        case contextFailed
        case tokenizeFailed
        case decodeFailed(Int32)
        case inputTooLong(Int)

        var description: String {
            switch self {
            case .loadFailed: "The cleanup model could not be loaded."
            case .contextFailed: "The cleanup model could not start."
            case .tokenizeFailed: "The transcript could not be prepared for cleanup."
            case .decodeFailed(let code): "The cleanup model stopped with error \(code)."
            case .inputTooLong(let count): "A piece of \(count) tokens is too long to clean."
            }
        }
    }

    /// Room for the prompt (system prompt plus a chunk of at most 1000 tokens) and its answer
    /// (at most 1.3 × the chunk + 32).
    static let contextLength: UInt32 = 4096

    private let model: OpaquePointer
    private let context: OpaquePointer
    private let vocab: OpaquePointer

    private static let backend: Void = {
        // llama.cpp logs model and device details to stderr by default; it never sees our logs.
        llama_log_set({ _, _, _ in }, nil)
        llama_backend_init()
    }()

    init(file: URL) throws {
        _ = Self.backend
        var modelParams = llama_model_default_params()
        modelParams.n_gpu_layers = -1  // all layers on Metal
        guard let model = llama_model_load_from_file(file.path(percentEncoded: false), modelParams) else {
            throw Failure.loadFailed
        }
        var contextParams = llama_context_default_params()
        contextParams.n_ctx = Self.contextLength
        contextParams.n_batch = Self.contextLength
        guard let context = llama_init_from_model(model, contextParams) else {
            llama_model_free(model)
            throw Failure.contextFailed
        }
        guard let vocab = llama_model_get_vocab(model) else {
            llama_free(context)
            llama_model_free(model)
            throw Failure.loadFailed
        }
        self.model = model
        self.context = context
        self.vocab = vocab
    }

    deinit {
        llama_free(context)
        llama_model_free(model)
    }

    /// Cleans the transcript chunk by chunk and joins the non-empty answers with spaces.
    func clean(_ transcript: String, style: CleanupStyle, stop: StopFlag) throws -> String {
        let chunks = S1MiniFormat.chunks(transcript) { (try? tokenize($0, special: false).count) ?? $0.utf8.count }
        var answers: [String] = []
        for chunk in chunks {
            if stop.isSet { throw CancellationError() }
            let answer = try generate(for: chunk, style: style, stop: stop)
            if !answer.isEmpty { answers.append(answer) }
        }
        return answers.joined(separator: " ")
    }

    /// One greedy pass over one chunk.
    private func generate(for chunk: String, style: CleanupStyle, stop: StopFlag) throws -> String {
        // The transcript is tokenised as plain text, so words that look like control tokens can't
        // steer the model; only the fixed prompt around it may contain them.
        let prompt = S1MiniFormat.prompt(for: "\u{0}", style: style)
        let parts = prompt.components(separatedBy: "\u{0}")
        let before = try tokenize(parts[0], special: true)
        let input = try tokenize(chunk, special: false)
        let after = try tokenize(parts[1], special: true)
        var tokens = before + input + after
        let maxNew = S1MiniFormat.maxNewTokens(forInputTokens: input.count)
        guard tokens.count + maxNew <= Int(llama_n_ctx(context)) else {
            throw Failure.inputTooLong(input.count)
        }

        llama_memory_clear(llama_get_memory(context), true)
        let sampler = llama_sampler_chain_init(llama_sampler_chain_default_params())
        defer { llama_sampler_free(sampler) }
        llama_sampler_chain_add(sampler, llama_sampler_init_greedy())

        let status = tokens.withUnsafeMutableBufferPointer { buffer in
            llama_decode(context, llama_batch_get_one(buffer.baseAddress, Int32(buffer.count)))
        }
        guard status == 0 else { throw Failure.decodeFailed(status) }

        var output: [UInt8] = []
        for _ in 0..<maxNew {
            if stop.isSet { throw CancellationError() }
            var token = llama_sampler_sample(sampler, context, -1)
            if llama_vocab_is_eog(vocab, token) { break }
            llama_sampler_accept(sampler, token)
            output += piece(token)
            let status = llama_decode(context, llama_batch_get_one(&token, 1))
            guard status == 0 else { throw Failure.decodeFailed(status) }
        }
        return S1MiniFormat.answer(from: String(decoding: output, as: UTF8.self))
    }

    private func tokenize(_ text: String, special: Bool) throws -> [llama_token] {
        let utf8 = Array(text.utf8)
        var tokens = [llama_token](repeating: 0, count: utf8.count + 8)
        let count = utf8.withUnsafeBufferPointer { bytes in
            bytes.withMemoryRebound(to: CChar.self) { chars in
                llama_tokenize(vocab, chars.baseAddress, Int32(chars.count), &tokens, Int32(tokens.count), false, special)
            }
        }
        guard count >= 0 else { throw Failure.tokenizeFailed }
        return Array(tokens.prefix(Int(count)))
    }

    private func piece(_ token: llama_token) -> [UInt8] {
        var buffer = [CChar](repeating: 0, count: 64)
        var count = llama_token_to_piece(vocab, token, &buffer, Int32(buffer.count), 0, false)
        if count < 0 {
            buffer = [CChar](repeating: 0, count: Int(-count))
            count = llama_token_to_piece(vocab, token, &buffer, Int32(buffer.count), 0, false)
        }
        return buffer.prefix(Int(max(count, 0))).map { UInt8(bitPattern: $0) }
    }
}
