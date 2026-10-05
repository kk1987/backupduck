import CoreImage
import Foundation
import Photos

/// Google Photos on Android 10 never backs up AVIF files, even when MediaStore
/// labels them `image/avif`, so the receiver could neither publish them in a
/// usable form nor ever see them verified in the cloud. AVIF photos are sent as
/// 10-bit HEIC in the source color space instead; EXIF, capture time and
/// orientation travel with the image properties.
enum AvifConversion {
  /// Primary media types the receiver cannot use; a source last received in
  /// one of them is prepared again in the converted format.
  static let supersededMediaTypes = ["image/avif"]
  /// 0.9 measured 52 dB PSNR against the decoded AVIF on a 20 MP, 10-bit
  /// Display P3 export; 0.95 gained 1.3 dB for 40% more bytes.
  static let quality = 0.9

  static func applies(to resource: PHAssetResource, role: String) -> Bool {
    role == "photo" && resource.uniformTypeIdentifier == "public.avif"
  }

  /// The original name with a `.heic` extension, e.g. `P1000919.heic`.
  static func filename(for original: String) -> String {
    let stem = (original as NSString).deletingPathExtension
    return (stem.isEmpty ? "photo" : stem) + ".heic"
  }

  /// Converts off the main actor and removes the AVIF export on success.
  static func convert(_ source: URL, to destination: URL) async throws {
    try await Task.detached(priority: .utility) {
      guard let image = CIImage(contentsOf: source, options: [.applyOrientationProperty: false])
      else { throw Bridge.Failure(code: "export_failed") }
      let space = image.colorSpace ?? CGColorSpace(name: CGColorSpace.displayP3)!
      let options: [CIImageRepresentationOption: Any] = [
        CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String):
          quality
      ]
      do {
        try CIContext().writeHEIF10Representation(
          of: image, to: destination, colorSpace: space, options: options)
      } catch {
        try? FileManager.default.removeItem(at: destination)
        throw Bridge.Failure(code: "export_failed")
      }
      try? FileManager.default.removeItem(at: source)
    }.value
  }
}
