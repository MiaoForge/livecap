import AVFoundation
import CoreGraphics

final class Probe: @unchecked Sendable {
    var tone: Process?
    var packets = 0
    var nonzero = 0
    var peak = 0
    var started = Date()
    var microphonePackets = 0
    var microphonePeak = 0
}

@main struct CaptureProbe {
    static func main() async {
        let includeMicrophone = CommandLine.arguments.contains("--microphone")
        let permission = requestAudioAccess(includeMicrophone ? 1 : 0)
        guard permission == 0 else {
            let location = permission == 2 ? "Microphone" : "Screen & System Audio Recording"
            fputs("Audio access denied. Allow the launching terminal in System Settings → Privacy & Security → \(location), then restart it.\n", stderr)
            exit(1)
        }
        print("Audio access granted. Capturing locally for 8 seconds; no recording or provider connection.")
        if includeMicrophone { print("Speak into the system default microphone during this test.") }
        let probe = Probe()
        DispatchQueue.global().asyncAfter(deadline: .now() + 20) {
            fputs("Capture probe timed out\n", stderr)
            exit(3)
        }
        let result: Int32 = await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                let pointer = Unmanaged.passUnretained(probe).toOpaque()
                let result = captureAudio(24000, CommandLine.arguments.contains("--microphone") ? 1 : 0, pointer, { context, source, bytes, count in
                    if source == 2 { return 0 }
                    let state = Unmanaged<Probe>.fromOpaque(context).takeUnretainedValue()
                    state.packets += 1
                    var peak = 0
                    for i in stride(from: 0, to: count, by: 2) {
                        peak = max(peak, abs(Int(Int16(bitPattern: UInt16(bytes[i]) | UInt16(bytes[i+1]) << 8))))
                    }
                    if source == 1 {
                        state.microphonePackets += 1
                        state.microphonePeak = max(state.microphonePeak, peak)
                    }
                    state.peak = max(state.peak, peak)
                    if peak > 0 { state.nonzero += 1 }
                    return 0
                }, { context in
                    let state = Unmanaged<Probe>.fromOpaque(context).takeUnretainedValue()
                    return Date().timeIntervalSince(state.started) > 8 ? 1 : 0
                }, { context in
                    let state = Unmanaged<Probe>.fromOpaque(context).takeUnretainedValue()
                    state.started = Date()
                    print("Capture started")
                    if CommandLine.arguments.contains("--tone") {
                        let tone = Process()
                        tone.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
                        tone.arguments = ["-v", "0.3", "/System/Library/Sounds/Glass.aiff"]
                        do { try tone.run(); state.tone = tone }
                        catch { fputs("Could not play test tone: \(error)\n", stderr); return 1 }
                    }
                    return 0
                })
                continuation.resume(returning: result)
            }
        }
        print("result=\(result), packets=\(probe.packets), nonzero=\(probe.nonzero), peak=\(probe.peak)")
        if includeMicrophone { print("microphone_packets=\(probe.microphonePackets), microphone_peak=\(probe.microphonePeak)") }
        if result != 0 || probe.nonzero == 0 || (includeMicrophone && probe.microphonePeak == 0) {
            fputs("No audible system audio was verified. Play audio, check capture permissions and output routing, then retry with --tone.\n", stderr)
            exit(1)
        }
    }
}
