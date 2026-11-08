import AVFoundation
import ScreenCaptureKit
import CoreGraphics

// Called from the user-initiated start command, before opening provider sockets.
@_cdecl("livecap_request_audio_access")
func requestAudioAccess() -> Int32 {
    CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() ? 0 : 1
}

typealias PacketCallback = @convention(c) (UnsafeMutableRawPointer, UnsafePointer<UInt8>, Int) -> Int32

final class AudioOutput: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    let context: UnsafeMutableRawPointer
    let packet: PacketCallback
    let format: AVAudioFormat
    let queue = DispatchQueue(label: "com.chenmiaoi.livecap.audio")
    var converter: AVAudioConverter?
    var lastPacket = Date()
    // Accessed only on queue; stream errors are forwarded onto it too.
    var failed = false

    init(context: UnsafeMutableRawPointer, rate: Double, packet: @escaping PacketCallback) {
        self.context = context
        self.packet = packet
        format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: rate, channels: 1, interleaved: true)!
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        queue.async { self.failed = true }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio else { return }
        process(sample)
    }

    func process(_ sample: CMSampleBuffer) {
        guard sample.isValid, !failed else { return }
        guard let description = sample.formatDescription,
              let streamDescription = CMAudioFormatDescriptionGetStreamBasicDescription(description),
              let inputFormat = AVAudioFormat(streamDescription: streamDescription) else { failed = true; return }
        guard let input = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(sample.numSamples)) else {
            failed = true
            return
        }
        input.frameLength = input.frameCapacity
        guard CMSampleBufferCopyPCMDataIntoAudioBufferList(sample, at: 0, frameCount: Int32(input.frameLength), into: input.mutableAudioBufferList) == noErr else {
            failed = true
            return
        }
        process(input)
    }

    func process(_ input: AVAudioPCMBuffer) {
        let inputFormat = input.format
        if converter == nil || converter?.inputFormat != inputFormat {
            converter = AVAudioConverter(from: inputFormat, to: format)
        }
        guard let converter,
              let output = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(ceil(Double(input.frameLength) * format.sampleRate / inputFormat.sampleRate)) + 32) else {
            failed = true
            return
        }
        var supplied = false
        var error: NSError?
        converter.convert(to: output, error: &error) { _, status in
            if supplied {
                status.pointee = .noDataNow
                return nil
            }
            supplied = true
            status.pointee = .haveData
            return input
        }
        guard error == nil else { failed = true; return }
        let buffer = output.audioBufferList.pointee.mBuffers
        if let data = buffer.mData, buffer.mDataByteSize > 0 {
            lastPacket = Date()
            failed = packet(context, data.assumingMemoryBound(to: UInt8.self), Int(buffer.mDataByteSize)) != 0
        }
    }
}

// Runs on the Rust capture worker, never on the app's main thread. Rust owns
// context until this call returns, after stopCapture and the callback queue drain.
@_cdecl("livecap_capture_audio")
func captureAudio(_ rate: UInt32, _ context: UnsafeMutableRawPointer,
                  _ packet: @escaping @convention(c) (UnsafeMutableRawPointer, UnsafePointer<UInt8>, Int) -> Int32,
                  _ stopped: @escaping @convention(c) (UnsafeMutableRawPointer) -> Int32,
                  _ ready: @escaping @convention(c) (UnsafeMutableRawPointer) -> Int32) -> Int32 {
    let done = DispatchSemaphore(value: 0)
    var result: Int32 = 0
    Task {
        let output = AudioOutput(context: context, rate: Double(rate), packet: packet)
        do {
            if output.queue.sync(execute: { stopped(context) != 0 }) { done.signal(); return }
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let display = content.displays.first else { result = 1; done.signal(); return }
            if output.queue.sync(execute: { stopped(context) != 0 }) { done.signal(); return }
            let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
            let configuration = SCStreamConfiguration()
            configuration.width = 2
            configuration.height = 2
            configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
            configuration.capturesAudio = true
            configuration.sampleRate = 48000
            configuration.channelCount = 1
            let stream = SCStream(filter: filter, configuration: configuration, delegate: output)
            try stream.addStreamOutput(output, type: .audio, sampleHandlerQueue: output.queue)
            // Detach output on every exit, including startup/shutdown errors.
            defer { try? stream.removeStreamOutput(output, type: .audio) }
            try await stream.startCapture()
            if output.queue.sync(execute: { ready(context) }) == 0 {
                while output.queue.sync(execute: { stopped(context) == 0 && !output.failed }) {
                    try await Task.sleep(nanoseconds: 10_000_000)
                    output.queue.sync {
                        if Date().timeIntervalSince(output.lastPacket) >= 0.1 && !output.failed {
                            let silence = [UInt8](repeating: 0, count: Int(rate) / 10 * 2)
                            output.lastPacket = Date()
                            output.failed = silence.withUnsafeBufferPointer { packet(context, $0.baseAddress!, $0.count) != 0 }
                        }
                    }
                }
            }
            try await stream.stopCapture()
            if output.queue.sync(execute: { output.failed }) { result = 2 }
        } catch {
            result = 1
        }
        // No callbacks may access Rust's context after returning.
        output.queue.sync {}
        done.signal()
    }
    done.wait()
    return result
}
