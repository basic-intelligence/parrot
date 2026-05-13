import Foundation

enum SpeechActivityTrimmer {
    static func trimForDictation(
        _ samples: [Float],
        sampleRate: Int = 16_000,
        frameMilliseconds: Int = 20,
        paddingMilliseconds: Int = 160,
        minimumSpeechMilliseconds: Int = 120
    ) -> [Float] {
        guard !samples.isEmpty else { return [] }

        let frameSize = max(1, sampleRate * frameMilliseconds / 1000)
        let frameCount = samples.count / frameSize
        guard frameCount > 0 else { return samples }

        var rmsValues: [Float] = []
        rmsValues.reserveCapacity(frameCount)

        for frame in 0..<frameCount {
            let start = frame * frameSize
            let end = min(samples.count, start + frameSize)
            var sum: Float = 0

            for sample in samples[start..<end] {
                sum += sample * sample
            }

            rmsValues.append(sqrt(sum / Float(max(1, end - start))))
        }

        let sorted = rmsValues.sorted()
        let noiseFloorIndex = max(0, min(sorted.count - 1, sorted.count / 10))
        let noiseFloor = sorted[noiseFloorIndex]
        let threshold = max(Float(0.008), noiseFloor * 3.0)

        guard let firstSpeech = rmsValues.firstIndex(where: { $0 >= threshold }),
              let lastSpeech = rmsValues.lastIndex(where: { $0 >= threshold })
        else {
            return []
        }

        let speechFrames = lastSpeech - firstSpeech + 1
        let minimumSpeechFrames = max(1, minimumSpeechMilliseconds / frameMilliseconds)
        guard speechFrames >= minimumSpeechFrames else { return [] }

        let paddingFrames = max(1, paddingMilliseconds / frameMilliseconds)
        let firstFrame = max(0, firstSpeech - paddingFrames)
        let lastFrame = min(frameCount - 1, lastSpeech + paddingFrames)

        let startSample = firstFrame * frameSize
        let endSample = min(samples.count, (lastFrame + 1) * frameSize)

        return Array(samples[startSample..<endSample])
    }
}
