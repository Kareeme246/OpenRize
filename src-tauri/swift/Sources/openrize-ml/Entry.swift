import Foundation

/// Protocol version, bumped whenever a request or reply shape changes so Rust
/// can refuse a stale sidecar instead of misreading it.
let protocolVersion = "1"

@main
struct OpenRizeML {
    static func main() async {
        if CommandLine.arguments.contains("--version") {
            print("openrize-ml \(protocolVersion)")
            return
        }

        // Replies own the real stdout. Frameworks (Create ML prints training
        // progress) write to fd 1 too, so fd 1 is pointed at stderr and the
        // protocol keeps a private duplicate of the original stdout.
        let protocolFd = dup(STDOUT_FILENO)
        dup2(STDERR_FILENO, STDOUT_FILENO)
        let output = FileHandle(fileDescriptor: protocolFd, closeOnDealloc: true)

        var engine = Engine()
        // One request at a time: the Rust worker never pipelines, and the
        // Foundation Model serves one generation per process anyway.
        while let line = readLine(strippingNewline: true) {
            if line.isEmpty { continue }
            let reply = await engine.handle(line: Data(line.utf8))
            var line = reply
            line.append(0x0A)
            output.write(line)
        }
    }
}

struct Engine {
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()
    private var embedding = Embedding()
    private var personal = PersonalModels()

    mutating func handle(line: Data) async -> Data {
        guard let envelope = try? decoder.decode(Envelope.self, from: line) else {
            return encode(Failure(id: -1, e: "malformed request"))
        }
        do {
            switch envelope.m {
            case "capabilities":
                return encode(Reply(id: envelope.id, r: capabilities()))
            case "embed":
                let params = try decoder.decode(WithParams<EmbedParams>.self, from: line).p
                return encode(Reply(id: envelope.id, r: embedding.embed(params.texts)))
            case "classify":
                let params = try decoder.decode(WithParams<ClassifyParams>.self, from: line).p
                let result = try await Language.classify(params)
                return encode(Reply(id: envelope.id, r: result))
            case "train":
                let params = try decoder.decode(WithParams<TrainParams>.self, from: line).p
                return encode(Reply(id: envelope.id, r: try personal.train(params)))
            case "personal_predict":
                let params = try decoder.decode(WithParams<PredictParams>.self, from: line).p
                return encode(Reply(id: envelope.id, r: try personal.predict(params)))
            case "ping":
                return encode(Reply(id: envelope.id, r: Empty()))
            default:
                return encode(Failure(id: envelope.id, e: "unknown method \(envelope.m)"))
            }
        } catch {
            return encode(Failure(id: envelope.id, e: String(describing: error)))
        }
    }

    private mutating func capabilities() -> CapabilitiesResult {
        let os = ProcessInfo.processInfo.operatingSystemVersion
        return CapabilitiesResult(
            llm: Language.availability(),
            embed: embedding.isAvailable,
            embedDimension: embedding.dimension,
            embedRevision: embedding.revision,
            personal: true,
            os: "\(os.majorVersion).\(os.minorVersion)",
            version: protocolVersion
        )
    }

    private func encode<T: Encodable>(_ value: T) -> Data {
        (try? encoder.encode(value)) ?? Data("{\"id\":-1,\"e\":\"encode failed\"}".utf8)
    }
}
