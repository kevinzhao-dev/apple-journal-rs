// Apple-framework adapter. Store access, CLI dispatch and mutations live in Rust.
import Foundation
import AppKit
import ImageIO
import UniformTypeIdentifiers
import LinkPresentation

func rtfToText(_ blob: Data?) -> String {
    guard let blob, let att = NSAttributedString(rtf: blob, documentAttributes: nil) else { return "" }
    return att.string.trimmingCharacters(in: .whitespacesAndNewlines)
}
struct BridgeError: Error { let message: String }
func perform(_ q: [String: Any]) throws -> [String: Any] {
    let text = q["text"] as? String ?? ""
    switch q["op"] as? String {
    case "decode":
        let d = Data(base64Encoded: text) ?? Data()
        return ["text": rtfToText(d), "valid": NSAttributedString(rtf: d, documentAttributes: nil) != nil]
    case "encode", "styled":
        let att = q["op"] as? String == "styled" ? attributed(q["lines"] as? [[String: Any]] ?? []) : NSAttributedString(string: text, attributes: [.font: NSFont.systemFont(ofSize: 12)])
        guard let data = att.rtf(from: NSRange(location: 0, length: att.length), documentAttributes: [:]) else { throw BridgeError(message: "could not encode RTF") }
        return ["data": data.base64EncodedString(), "text": rtfToText(data)]
    case "link-encode":
        guard let u = URL(string: text), let scheme = u.scheme, ["http", "https"].contains(scheme), u.host != nil else { throw BridgeError(message: "--link needs an absolute http(s) URL") }
        let md = LPLinkMetadata(); md.originalURL = u; md.url = u; md.title = q["title"] as? String
        return ["data": try NSKeyedArchiver.archivedData(withRootObject: md, requiringSecureCoding: true).base64EncodedString()]
    case "link-decode":
        guard let d = Data(base64Encoded: text), let md = try? NSKeyedUnarchiver.unarchivedObject(ofClass: LPLinkMetadata.self, from: d) else { return [:] }
        var result: [String: Any] = [:]
        if let u = md.url ?? md.originalURL { result["url"] = u.absoluteString }
        if let t = md.title { result["link_title"] = t }
        return result
    case "resize":
        let dest = q["dest"] as! String
        if let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: text) as CFURL, nil),
           let p = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
           let w = p[kCGImagePropertyPixelWidth] as? Int, let h = p[kCGImagePropertyPixelHeight] as? Int, max(w,h) > 2830,
           let image = CGImageSourceCreateThumbnailAtIndex(src, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceThumbnailMaxPixelSize: 2830, kCGImageSourceCreateThumbnailWithTransform: true] as CFDictionary) {
            let type = UTType(filenameExtension: URL(fileURLWithPath: dest).pathExtension)?.identifier ?? UTType.jpeg.identifier
            if let output = CGImageDestinationCreateWithURL(URL(fileURLWithPath: dest) as CFURL, type as CFString, 1, nil) {
                CGImageDestinationAddImage(output, image, [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary)
                if CGImageDestinationFinalize(output) { return [:] }
            }
        }
        if FileManager.default.fileExists(atPath: dest) { try FileManager.default.removeItem(atPath: dest) }
        try FileManager.default.copyItem(atPath: text, toPath: dest)
        return [:]
    default: throw BridgeError(message: "unknown bridge operation")
    }
}
@_cdecl("journal_bridge")
public func journalBridge(_ input: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    autoreleasepool {
        let result: [String: Any]
        do {
            let q = try JSONSerialization.jsonObject(with: Data(String(cString: input).utf8)) as! [String: Any]
            result = try perform(q)
        } catch { result = ["error": (error as? BridgeError)?.message ?? String(describing: error)] }
        guard let data = try? JSONSerialization.data(withJSONObject: result), let s = String(data: data, encoding: .utf8) else { return nil }
        return strdup(s)
    }
}

// Rust supplies the parsed lines and spans. Only native typography lives here.
private func attributed(_ lines: [[String: Any]]) -> NSAttributedString {
    let out = NSMutableAttributedString()
    let plain = NSFont.systemFont(ofSize: 12)
    func appendSpans(_ line: [String: Any], _ style: NSParagraphStyle?) {
        for sp in line["spans"] as? [[String: Any]] ?? [] {
            var font = sp["bold"] as? Bool == true ? NSFont.boldSystemFont(ofSize: 12) : plain
            if sp["italic"] as? Bool == true { font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask) }
            var attrs: [NSAttributedString.Key: Any] = [.font: font]
            if sp["strike"] as? Bool == true { attrs[.strikethroughStyle] = 1 }
            if let style { attrs[.paragraphStyle] = style }
            out.append(NSAttributedString(string: sp["text"] as? String ?? "", attributes: attrs))
        }
    }
    var index = 0, blanks = 0
    while index < lines.count {
        let kind = lines[index]["kind"] as? String ?? "body"
        if kind == "bullet" || kind == "ordered" {
            let list = NSTextList(markerFormat: kind == "ordered" ? .decimal : .disc, options: 0)
            let style = NSMutableParagraphStyle(); style.textLists = [list]; style.headIndent = 24; style.firstLineHeadIndent = 12
            var item = 0
            while index < lines.count, lines[index]["kind"] as? String == kind {
                item += 1
                out.append(NSAttributedString(string: "\t\(list.marker(forItemNumber: item))\t", attributes: [.font: plain, .paragraphStyle: style]))
                appendSpans(lines[index], style)
                out.append(NSAttributedString(string: "\n", attributes: [.font: plain, .paragraphStyle: style]))
                index += 1
            }
            blanks = 0; continue
        }
        let line = lines[index]; index += 1
        let spans = line["spans"] as? [[String: Any]] ?? []
        if spans.allSatisfy({ ($0["text"] as? String ?? "").trimmingCharacters(in: .whitespaces).isEmpty }) {
            blanks += 1
            if blanks > 1 || out.length == 0 { continue }
            out.append(NSAttributedString(string: "\n", attributes: [.font: plain])); continue
        }
        blanks = 0
        var style: NSMutableParagraphStyle? = nil
        if kind == "quote" { style = NSMutableParagraphStyle(); style?.headIndent = 24; style?.firstLineHeadIndent = 24 }
        appendSpans(line, style)
        out.append(NSAttributedString(string: "\n", attributes: [.font: plain]))
    }
    while out.length > 0, out.string.hasSuffix("\n") { out.deleteCharacters(in: NSRange(location: out.length - 1, length: 1)) }
    return out
}
