import AVFoundation
import ScreenCaptureKit
import CoreGraphics

private final class PermissionReply: @unchecked Sendable {
    let completed = DispatchSemaphore(value: 0)
    // The semaphore orders the completion write before the worker's read.
    var granted = false
}

// Called on a blocking worker so the app remains responsive to privacy prompts.
@_cdecl("livecap_request_audio_access")
func requestAudioAccess(_ includeMicrophone: Int32) -> Int32 {
    guard CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() else { return 1 }
    guard includeMicrophone != 0 else { return 0 }
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized: return 0
    case .notDetermined:
        let reply = PermissionReply()
        AVCaptureDevice.requestAccess(for: .audio) { granted in
            reply.granted = granted
            reply.completed.signal()
        }
        reply.completed.wait()
        return reply.granted ? 0 : 2
    default: return 2
    }
}

// Source 0: system PCM; 1: microphone PCM; 2: a 20 ms mixer-clock tick.
typealias PacketCallback = @convention(c) (UnsafeMutableRawPointer, Int32, UnsafePointer<UInt8>, Int) -> Int32

final class AudioOutput: NSObject, SCStreamOutput, SCStreamDelegate, AVCaptureAudioDataOutputSampleBufferDelegate, @unchecked Sendable {
    let context: UnsafeMutableRawPointer
    let packet: PacketCallback
    let format: AVAudioFormat
    let queue: DispatchQueue
    let source: Int32
    var converter: AVAudioConverter?
    var lastPacket = Date()
    // Accessed only on queue; stream errors are forwarded onto it too.
    var failed = false

    init(context: UnsafeMutableRawPointer, rate: Double, packet: @escaping PacketCallback,
         source: Int32 = 0, queue: DispatchQueue = DispatchQueue(label: "com.chenmiaoi.livecap.audio")) {
        self.context = context
        self.packet = packet
        self.source = source
        self.queue = queue
        format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: rate, channels: 1, interleaved: true)!
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        queue.async { self.failed = true }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio else { return }
        process(sample)
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sample: CMSampleBuffer, from connection: AVCaptureConnection) {
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
            failed = packet(context, source, data.assumingMemoryBound(to: UInt8.self), Int(buffer.mDataByteSize)) != 0
        }
    }
}

private final class MicrophoneCapture {
    private let session = AVCaptureSession()
    private let output = AVCaptureAudioDataOutput()

    init(receiver: AudioOutput) throws {
        guard let device = AVCaptureDevice.default(for: .audio) else { throw Self.unavailable() }
        let input = try AVCaptureDeviceInput(device: device)
        guard session.canAddInput(input) else { throw Self.unavailable() }
        session.addInput(input)
        guard session.canAddOutput(output) else { throw Self.unavailable() }
        output.audioSettings = [AVFormatIDKey: kAudioFormatLinearPCM]
        output.setSampleBufferDelegate(receiver, queue: receiver.queue)
        session.addOutput(output)
    }

    func start() throws {
        // This synchronous helper runs on the native capture task, never the UI thread.
        session.startRunning()
        guard session.isRunning else { throw Self.unavailable() }
    }

    func stop() {
        output.setSampleBufferDelegate(nil, queue: nil)
        session.stopRunning()
    }

    private static func unavailable() -> NSError {
        NSError(domain: "LivecapAudio", code: 3, userInfo: [NSLocalizedDescriptionKey: "The default microphone is unavailable."])
    }
}

// Runs on the Rust capture worker, never on the app's main thread. Rust owns
// context until this call returns, after stopCapture and the callback queue drain.
@_cdecl("livecap_capture_audio")
func captureAudio(_ rate: UInt32, _ includeMicrophone: Int32, _ context: UnsafeMutableRawPointer,
                  _ packet: @escaping @convention(c) (UnsafeMutableRawPointer, Int32, UnsafePointer<UInt8>, Int) -> Int32,
                  _ stopped: @escaping @convention(c) (UnsafeMutableRawPointer) -> Int32,
                  _ ready: @escaping @convention(c) (UnsafeMutableRawPointer) -> Int32) -> Int32 {
    let done = DispatchSemaphore(value: 0)
    var result: Int32 = 0
    Task {
        let output = AudioOutput(context: context, rate: Double(rate), packet: packet)
        let microphone = AudioOutput(context: context, rate: Double(rate), packet: packet, source: 1, queue: output.queue)
        var microphoneCapture: MicrophoneCapture?
        defer {
            microphoneCapture?.stop()
            // Drain both microphone and stream callbacks before returning Rust's context.
            output.queue.sync {}
            done.signal()
        }
        do {
            if output.queue.sync(execute: { stopped(context) != 0 }) { return }
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let display = content.displays.first else { result = 1; return }
            if output.queue.sync(execute: { stopped(context) != 0 }) { return }
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
            if includeMicrophone != 0 {
                do {
                    let capture = try MicrophoneCapture(receiver: microphone)
                    microphoneCapture = capture
                    try capture.start()
                } catch {
                    result = 3
                    try? await stream.stopCapture()
                    throw error
                }
            }
            var nextMix = DispatchTime.now().uptimeNanoseconds + 20_000_000
            if output.queue.sync(execute: { ready(context) }) == 0 {
                while output.queue.sync(execute: { stopped(context) == 0 && !output.failed && !microphone.failed }) {
                    try await Task.sleep(nanoseconds: 10_000_000)
                    output.queue.sync {
                        if includeMicrophone != 0 {
                            if DispatchTime.now().uptimeNanoseconds >= nextMix && !output.failed && !microphone.failed {
                                var tick: UInt8 = 0
                                output.failed = withUnsafePointer(to: &tick) { packet(context, 2, $0, 0) != 0 }
                                nextMix += 20_000_000
                            }
                        } else if Date().timeIntervalSince(output.lastPacket) >= 0.1 && !output.failed {
                            let silence = [UInt8](repeating: 0, count: Int(rate) / 10 * 2)
                            output.lastPacket = Date()
                            output.failed = silence.withUnsafeBufferPointer { packet(context, 0, $0.baseAddress!, $0.count) != 0 }
                        }
                    }
                }
            }
            try await stream.stopCapture()
            if output.queue.sync(execute: { output.failed }) { result = 2 }
            if output.queue.sync(execute: { microphone.failed }) { result = 3 }
        } catch {
            if result == 0 { result = 1 }
        }
    }
    done.wait()
    return result
}
