import Foundation

#if canImport(FoundationModels)
import FoundationModels
#endif

/// Label used for the "not part of any listed project" answer. It is a real
/// answer with its own votes, not a missing value.
let noProjectLabel = "No project"

/// T2: Apple's on-device Foundation Model with guided generation. A
/// `DynamicGenerationSchema` restricts `category` and `project` to the live
/// lists Rust sends, so the model cannot name something that doesn't exist.
enum Language {
    static func availability() -> String {
        #if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            switch SystemLanguageModel.default.availability {
            case .available:
                return "available"
            case .unavailable(.appleIntelligenceNotEnabled):
                return "appleIntelligenceNotEnabled"
            case .unavailable(.modelNotReady):
                return "modelNotReady"
            case .unavailable(.deviceNotEligible):
                return "deviceNotEligible"
            case .unavailable:
                return "unknown"
            }
        }
        return "unsupportedOS"
        #else
        return "unsupportedOS"
        #endif
    }

    static func classify(_ params: ClassifyParams) async throws -> ClassifyResult {
        #if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            return try await FoundationClassifier(params).run()
        }
        #endif
        throw SidecarError("the on-device language model needs macOS 26 or later")
    }
}

/// Distinct display names for a choice list. Two categories can share a name
/// (one archived and recreated, say); the schema needs unique strings.
func uniqueNames(_ choices: [Choice]) -> [(name: String, id: String)] {
    var seen: [String: Int] = [:]
    return choices.map { choice in
        let base = choice.name.trimmingCharacters(in: .whitespacesAndNewlines)
        let count = (seen[base] ?? 0) + 1
        seen[base] = count
        return (count == 1 ? base : "\(base) (\(count))", choice.id)
    }
}

#if canImport(FoundationModels)
@available(macOS 26.0, *)
struct FoundationClassifier {
    let params: ClassifyParams
    let categories: [(name: String, id: String)]
    let projects: [(name: String, id: String)]?

    init(_ params: ClassifyParams) {
        self.params = params
        self.categories = uniqueNames(params.categories)
        self.projects = params.projects.map { uniqueNames($0) }
    }

    func run() async throws -> ClassifyResult {
        guard !categories.isEmpty else { throw SidecarError("no categories to choose from") }
        guard case .available = SystemLanguageModel.default.availability else {
            throw SidecarError("model unavailable: \(Language.availability())")
        }

        let started = Date()
        let schema = try makeSchema()
        let samples = max(1, min(params.samples ?? 1, 5))
        var categoryVotes: [String: Int] = [:]
        var projectVotes: [String: Int] = [:]
        var description: String?

        for _ in 0..<samples {
            // A fresh session per sample: each call is independent, and the
            // transcript never grows past one exchange.
            let session = LanguageModelSession(instructions: instructions())
            let options: GenerationOptions
            if let temperature = params.temperature {
                options = GenerationOptions(temperature: temperature, maximumResponseTokens: 200)
            } else {
                options = GenerationOptions(sampling: .greedy, maximumResponseTokens: 200)
            }
            let response = try await session.respond(to: prompt(), schema: schema, options: options)
            let content = response.content

            let categoryName = try content.value(String.self, forProperty: "category")
            if let id = categories.first(where: { $0.name == categoryName })?.id {
                categoryVotes[id, default: 0] += 1
            }
            if let projects {
                let projectName = try content.value(String.self, forProperty: "project")
                let id = projects.first(where: { $0.name == projectName })?.id ?? "none"
                projectVotes[id, default: 0] += 1
            }
            if description == nil {
                description = try? content.value(String.self, forProperty: "description")
            }
        }

        return ClassifyResult(
            categoryVotes: categoryVotes,
            projectVotes: projects == nil ? nil : projectVotes,
            description: description?.trimmingCharacters(in: .whitespacesAndNewlines),
            samples: samples,
            latencyMs: Int(Date().timeIntervalSince(started) * 1000)
        )
    }

    private func makeSchema() throws -> GenerationSchema {
        var properties: [DynamicGenerationSchema.Property] = [
            .init(
                name: "description",
                description: "One short past-tense sentence, at most 12 words, saying what the person worked on.",
                schema: DynamicGenerationSchema(type: String.self)
            ),
            .init(
                name: "category",
                description: "The category that best describes the work.",
                schema: DynamicGenerationSchema(name: "Category", anyOf: categories.map(\.name))
            ),
        ]
        if let projects {
            properties.append(
                .init(
                    name: "project",
                    description: "The project this work belongs to, or \"\(noProjectLabel)\".",
                    schema: DynamicGenerationSchema(
                        name: "Project", anyOf: projects.map(\.name) + [noProjectLabel])
                ))
        }
        let root = DynamicGenerationSchema(name: "ActivityLabel", properties: properties)
        return try GenerationSchema(root: root, dependencies: [])
    }

    private func instructions() -> String {
        var text = """
            You label blocks of computer activity for a private, on-device time tracker. \
            Each block lists the apps used, their share of the time, and window titles or sites.
            Choose the category that best describes the work in the block.
            """
        if projects != nil {
            text += """
                \nChoose the project the work belongs to. Answer "\(noProjectLabel)" unless the \
                activity clearly matches one of the listed projects.
                """
        }
        text += "\nWrite a description that only uses details present in the activity."
        text += "\n\nCategories:"
        for (choice, unique) in zip(params.categories, categories) {
            text += "\n- \(unique.name)" + detail(choice)
        }
        if let projectChoices = params.projects, let projects {
            text += "\n\nProjects:"
            for (choice, unique) in zip(projectChoices, projects) {
                text += "\n- \(unique.name)" + detail(choice)
            }
        }
        if let custom = params.customInstructions?.trimmingCharacters(in: .whitespacesAndNewlines),
            !custom.isEmpty
        {
            text += "\n\nThe user's own instructions:\n\(custom)"
        }
        return text
    }

    private func detail(_ choice: Choice) -> String {
        let parts = [choice.description, choice.hint]
            .compactMap { $0?.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        return parts.isEmpty ? "" : ": " + parts.joined(separator: " ")
    }

    private func prompt() -> String {
        var text = ""
        if let examples = params.examples, !examples.isEmpty {
            text += "Blocks the user already labeled:\n"
            for example in examples {
                text += "\n\(example.features)\n=> Category: \(example.category)"
                if projects != nil {
                    text += ", Project: \(example.project ?? noProjectLabel)"
                }
                text += "\n"
            }
            text += "\n"
        }
        text += "Block to label:\n\(params.features)"
        return text
    }
}
#endif
