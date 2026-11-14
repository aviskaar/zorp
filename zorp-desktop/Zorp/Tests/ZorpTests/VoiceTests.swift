#if canImport(Testing)
import Foundation
import Testing
@testable import ZorpKit

@Test func wavHeaderDescribesSixteenKilohertzMonoPCM() {
    let wav = Wav.encode([0, 1, -1, 32767])
    #expect(wav.count == 44 + 8)
    #expect(String(decoding: wav[0..<4], as: UTF8.self) == "RIFF")
    #expect(String(decoding: wav[8..<12], as: UTF8.self) == "WAVE")
    let rate = wav[24..<28].withUnsafeBytes { $0.loadUnaligned(as: UInt32.self) }
    #expect(UInt32(littleEndian: rate) == 16_000)
    let dataBytes = wav[40..<44].withUnsafeBytes { $0.loadUnaligned(as: UInt32.self) }
    #expect(UInt32(littleEndian: dataBytes) == 8)
    // -1 as little-endian 16-bit two's complement.
    #expect(wav[48] == 0xFF && wav[49] == 0xFF)
}

@Test func aGuessIsReplacedAndAKeptStretchIsNot() {
    var t = LiveTranscript()
    t.guess("hello")
    t.guess("hello world")
    #expect(t.text == "hello world")
    t.commit("hello world.", through: 64_000)
    #expect(t.windowStart == 64_000)
    t.guess("how are")
    #expect(t.text == "hello world. how are")
    t.guess("how are you")
    t.commitGuess(through: 80_000)
    #expect(t.text == "hello world. how are you")
    #expect(t.windowStart == 80_000)
}

@Test func aStretchIsKeptAtAPauseAfterFourSecondsOrAtFifteen() {
    let rate = LiveWindow.sampleRate
    let loud = [Int16](repeating: 3000, count: rate * 5)
    let pause = [Int16](repeating: 10, count: rate)
    // Speech still going: not kept yet.
    #expect(!LiveWindow.shouldKeep(loud))
    // A pause before four seconds: too short to keep.
    #expect(!LiveWindow.shouldKeep(Array(loud.prefix(rate * 2)) + pause))
    // A pause after four seconds: kept.
    #expect(LiveWindow.shouldKeep(loud + pause))
    // Fifteen seconds of talking: kept regardless.
    #expect(LiveWindow.shouldKeep([Int16](repeating: 3000, count: LiveWindow.maxSamples)))
}

@Test func theSampleBufferHandsBackTheRangeAsked() {
    let buffer = SampleBuffer()
    [Int16](0..<10).withUnsafeBufferPointer { buffer.append($0) }
    #expect(buffer.count == 10)
    #expect(buffer.samples(from: 4, to: 7) == [4, 5, 6])
    #expect(buffer.samples(from: 8) == [8, 9])
    #expect(buffer.samples(from: 12).isEmpty)
    buffer.clear()
    #expect(buffer.count == 0)
}
#endif
