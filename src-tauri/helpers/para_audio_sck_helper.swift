#!/usr/bin/env swift
//
// para_audio_sck_helper — macOS ScreenCaptureKit audio capture helper.
//
// Captures system audio output via ScreenCaptureKit and writes raw i16 PCM
// (interleaved stereo, 48 kHz) to stdout. The main Para-audio Rust process
// reads from this process's stdout.
//
// No audio is written to disk.
//
// Requirements:
// - macOS 13+ (ScreenCaptureKit)
// - Screen Recording permission (System Settings > Privacy & Security > Screen Recording)
//
// Build:
//   swiftc -O -o para_audio_sck_helper para_audio_sck_helper.swift \
//     -framework ScreenCaptureKit -framework CoreMedia -framework AVFoundation
//
// Reference:
//   https://developer.apple.com/videos/play/wwdc2022/10156/
//   https://developer.apple.com/documentation/screencapturekit

import Foundation
import ScreenCaptureKit
import CoreMedia
import AVFoundation

// MARK: - Stream output delegate

class AudioOutputHandler: NSObject, SCStreamOutput {
    let stdout = FileHandle.standardOutput

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio else { return }
        guard let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

        var length = 0
        var dataPointer: UnsafeMutablePointer<Int8>?
        let status = CMBlockBufferGetDataPointer(blockBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &dataPointer)

        guard status == kCMBlockBufferNoErr, let ptr = dataPointer, length > 0 else { return }

        // Get audio format description
        guard let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer) else { return }
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc)?.pointee

        // Convert to i16 PCM if the source is float32
        if let asbd = asbd, asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0 {
            // Float32 -> Int16 conversion
            let floatCount = length / MemoryLayout<Float>.size
            let floatBuffer = UnsafeBufferPointer(start: UnsafeRawPointer(ptr).bindMemory(to: Float.self, capacity: floatCount), count: floatCount)

            var i16Data = Data(count: floatCount * 2)
            i16Data.withUnsafeMutableBytes { rawBuf in
                let i16Buf = rawBuf.bindMemory(to: Int16.self)
                for i in 0..<floatCount {
                    let clamped = max(-1.0, min(1.0, floatBuffer[i]))
                    i16Buf[i] = Int16(clamped * Float(Int16.max))
                }
            }
            stdout.write(i16Data)
        } else {
            // Already i16 or similar — write raw
            let data = Data(bytes: ptr, count: length)
            stdout.write(data)
        }
    }
}

// MARK: - Main

func main() async {
    // Check for Screen Recording permission by requesting shareable content
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)

        guard let display = content.displays.first else {
            fputs("Error: No displays found.\n", stderr)
            exit(1)
        }

        // Create a filter that captures the entire display audio (exclude our own app)
        let currentApp = content.applications.first { $0.bundleIdentifier == Bundle.main.bundleIdentifier }
        let filter: SCContentFilter
        if let app = currentApp {
            filter = SCContentFilter(display: display, excludingApplications: [app], exceptingWindows: [])
        } else {
            filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        }

        // Configure for audio-only capture
        let config = SCStreamConfiguration()
        config.capturesAudio = true
        config.sampleRate = 48000
        config.channelCount = 2
        // Disable video capture (we only want audio)
        config.width = 1
        config.height = 1
        config.minimumFrameInterval = CMTime(value: 1, timescale: 1) // 1 FPS minimum

        let stream = SCStream(filter: filter, configuration: config, delegate: nil)
        let handler = AudioOutputHandler()

        try stream.addStreamOutput(handler, type: .audio, sampleHandlerQueue: DispatchQueue(label: "audio-capture"))

        try await stream.startCapture()
        fputs("ScreenCaptureKit: audio capture started (48kHz stereo i16 PCM -> stdout)\n", stderr)

        // Run until killed by parent process
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            signal(SIGTERM) { _ in
                // Parent sent SIGTERM — exit cleanly
                exit(0)
            }
            signal(SIGINT) { _ in
                exit(0)
            }
            // Block forever (until signal)
            dispatchMain()
        }
    } catch {
        fputs("ScreenCaptureKit error: \(error.localizedDescription)\n", stderr)
        fputs("Ensure Screen Recording permission is granted in System Settings > Privacy & Security > Screen Recording.\n", stderr)
        exit(1)
    }
}

if #available(macOS 13.0, *) {
    Task {
        await main()
    }
    dispatchMain()
} else {
    fputs("Error: macOS 13+ required for ScreenCaptureKit audio capture.\n", stderr)
    exit(1)
}
