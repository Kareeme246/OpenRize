import CoreML
import CreateML
import Foundation
import NaturalLanguage

/// T1 personal classifier: a Create ML `MLTextClassifier` trained on the
/// user's own approved entries. Training takes milliseconds for hundreds of
/// rows, so it runs inline; Rust decides when to retrain and whether the new
/// model is good enough to replace the old one.
struct PersonalModels {
    private var cache: [String: NLModel] = [:]

    mutating func train(_ params: TrainParams) throws -> TrainResult {
        let started = Date()
        let rows = try readRows(at: params.dataset)
        var training: [String: [String]] = [:]
        var holdout: [TrainRow] = []
        for row in rows {
            if row.holdout {
                holdout.append(row)
            } else {
                training[row.label, default: []].append(row.text)
            }
        }
        guard training.count >= 2 else {
            throw SidecarError("need at least two labels to train, got \(training.count)")
        }

        let parameters = MLTextClassifier.ModelParameters(
            validation: .none,
            algorithm: .maxEnt(revision: 1),
            language: .english
        )
        let classifier = try MLTextClassifier(trainingData: training, parameters: parameters)

        var accuracy: Double?
        if !holdout.isEmpty {
            let predictions = try classifier.predictions(from: holdout.map(\.text))
            let correct = zip(predictions, holdout).filter { $0 == $1.label }.count
            accuracy = Double(correct) / Double(holdout.count)
        }

        let outDir = URL(fileURLWithPath: params.out)
        let fileManager = FileManager.default
        try fileManager.createDirectory(
            at: outDir.deletingLastPathComponent(), withIntermediateDirectories: true)
        // Write the uncompiled model to a private scratch directory: Create ML
        // treats an existing directory as "no file name given".
        let scratch = fileManager.temporaryDirectory
            .appendingPathComponent("openrize-ml-\(UUID().uuidString)", isDirectory: true)
        try fileManager.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? fileManager.removeItem(at: scratch) }
        let source = scratch.appendingPathComponent("Personal.mlmodel")
        try classifier.write(to: source)
        let compiled = try MLModel.compileModel(at: source)
        if fileManager.fileExists(atPath: outDir.path) {
            try fileManager.removeItem(at: outDir)
        }
        try fileManager.moveItem(at: compiled, to: outDir)
        cache[outDir.path] = nil

        return TrainResult(
            path: outDir.path,
            trainExamples: training.values.reduce(0) { $0 + $1.count },
            holdoutExamples: holdout.count,
            holdoutAccuracy: accuracy,
            labels: training.keys.sorted(),
            trainMs: Int(Date().timeIntervalSince(started) * 1000)
        )
    }

    mutating func predict(_ params: PredictParams) throws -> PredictResult {
        let model = try load(params.model)
        let hypotheses = params.texts.map { text in
            model.predictedLabelHypotheses(for: text, maximumCount: 10)
        }
        return PredictResult(hypotheses: hypotheses)
    }

    private mutating func load(_ path: String) throws -> NLModel {
        if let model = cache[path] { return model }
        let model = try NLModel(contentsOf: URL(fileURLWithPath: path))
        cache[path] = model
        return model
    }

    private func readRows(at path: String) throws -> [TrainRow] {
        let contents = try String(contentsOfFile: path, encoding: .utf8)
        let decoder = JSONDecoder()
        return try contents.split(separator: "\n").map { line in
            try decoder.decode(TrainRow.self, from: Data(line.utf8))
        }
    }
}
