import AVFoundation

private final class Packets {
    var samples: [Int16] = []
    var reject = false
    var sources: Set<Int32> = []
}

private func receive(_ context: UnsafeMutableRawPointer, _ source: Int32, _ bytes: UnsafePointer<UInt8>, _ count: Int) -> Int32 {
    let packets = Unmanaged<Packets>.fromOpaque(context).takeUnretainedValue()
    packets.sources.insert(source)
    for offset in stride(from: 0, to: count, by: 2) {
        packets.samples.append(Int16(bitPattern: UInt16(bytes[offset]) | UInt16(bytes[offset + 1]) << 8))
    }
    return packets.reject ? 1 : 0
}

@main
struct AudioCaptureTests {
    static func main() {
        for rate in [16000.0, 24000.0] {
            for channels: AVAudioChannelCount in [1, 2] {
                let packets = Packets()
                let context = Unmanaged.passUnretained(packets).toOpaque()
                let source: Int32 = channels == 1 ? 0 : 1
                let output = AudioOutput(context: context, rate: rate, packet: receive, source: source)
                let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48000, channels: channels, interleaved: false)!
                let input = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4800)!
                input.frameLength = 4800
                for channel in 0..<Int(channels) {
                    for frame in 0..<4800 {
                        input.floatChannelData![channel][frame] = Float(sin(Double(frame) * 2 * .pi * 440 / 48000) * 0.5)
                    }
                }
                // Exercise the CMSampleBuffer extraction used by ScreenCaptureKit too.
                var sample: CMSampleBuffer?
                precondition(CMAudioSampleBufferCreateWithPacketDescriptions(
                    allocator: kCFAllocatorDefault, dataBuffer: nil, dataReady: false,
                    makeDataReadyCallback: nil, refcon: nil,
                    formatDescription: format.formatDescription, sampleCount: Int(input.frameLength),
                    presentationTimeStamp: .zero, packetDescriptions: nil,
                    sampleBufferOut: &sample) == noErr)
                precondition(CMSampleBufferSetDataBufferFromAudioBufferList(
                    sample!, blockBufferAllocator: kCFAllocatorDefault,
                    blockBufferMemoryAllocator: kCFAllocatorDefault, flags: 0,
                    bufferList: input.audioBufferList) == noErr)
                precondition(CMSampleBufferSetDataReady(sample!) == noErr)
                for _ in 0..<10 { output.process(sample!) }
                precondition(!output.failed, "Conversion failed")
                precondition(packets.sources == [source], "System and microphone source labels must be preserved")
                precondition(abs(Double(packets.samples.count) - rate) < rate * 0.02, "Incorrect output duration: \(packets.samples.count) samples at \(rate) Hz, \(channels) channels")
                let peak = packets.samples.map { abs(Int($0)) }.max()!
                precondition(peak > 12000 && peak < 20000, "PCM16 amplitude is incorrect")
                let previousCount = packets.samples.count
                for _ in 0..<100 { output.process(input) }
                // AVAudioConverter retains a bounded resampling tail between calls.
                // Ten seconds of input must not accumulate timeline drift.
                precondition(abs(Double(packets.samples.count - previousCount) - rate * 10) < rate * 0.02, "Resampling accumulated timeline drift")
                for channel in 0..<Int(channels) {
                    input.floatChannelData![channel].initialize(repeating: 0, count: 4800)
                }
                packets.samples.removeAll()
                for _ in 0..<3 { output.process(input) }
                precondition(packets.samples.suffix(Int(rate / 10)).allSatisfy { $0 == 0 }, "Silence must remain silent")
                packets.reject = true
                output.process(input)
                precondition(output.failed, "Consumer failure must stop capture")
            }
        }
        print("macOS PCM conversion tests passed (16/24 kHz, mono/stereo, consumer failure)")
    }
}
