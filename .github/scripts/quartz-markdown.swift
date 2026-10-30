import AppKit
import CoreGraphics
import CoreText
import Darwin
import Foundation
import PDFKit

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

guard CommandLine.arguments.count == 4 else {
    fail("usage: quartz-markdown.swift INPUT.md OUTPUT.pdf TITLE")
}

let inputURL = URL(fileURLWithPath: CommandLine.arguments[1])
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])
let title = CommandLine.arguments[3]

do {
    // Keep the registered Markdown source verbatim: no image fetching, link
    // resolution, markup rewriting, or substitution of source content.
    let markdown = try String(contentsOf: inputURL, encoding: .utf8)
    let font = NSFont.monospacedSystemFont(ofSize: 9, weight: .regular)
    let attributed = NSAttributedString(string: markdown, attributes: [.font: font])
    let framesetter = CTFramesetterCreateWithAttributedString(attributed as CFAttributedString)

    let page = CGRect(x: 0, y: 0, width: 612, height: 792)
    let content = CGRect(x: 54, y: 54, width: 504, height: 684)
    let scale: CGFloat = 3
    let pixelWidth = Int(page.width * scale)
    let pixelHeight = Int(page.height * scale)
    let temporaryURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("tzim-fixed-quartz-\(UUID().uuidString).pdf")

    let document = PDFDocument()
    document.documentAttributes = [
        PDFDocumentAttribute.titleAttribute: title,
        PDFDocumentAttribute.authorAttribute: "tzim fixed #57 measurement",
        PDFDocumentAttribute.creatorAttribute: "AppKit/Quartz CoreText raster pages + PDFKit",
    ]

    var location = 0
    while location < attributed.length {
        guard let bitmap = CGContext(
            data: nil,
            width: pixelWidth,
            height: pixelHeight,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            fail("Quartz could not create a page bitmap context")
        }

        bitmap.scaleBy(x: scale, y: scale)
        bitmap.translateBy(x: 0, y: page.height)
        bitmap.scaleBy(x: 1, y: -1)
        bitmap.setFillColor(NSColor.white.cgColor)
        bitmap.fill(page)
        bitmap.textMatrix = .identity

        let path = CGPath(rect: content, transform: nil)
        let frame = CTFramesetterCreateFrame(
            framesetter,
            CFRange(location: location, length: 0),
            path,
            nil
        )
        CTFrameDraw(frame, bitmap)
        let visible = CTFrameGetVisibleStringRange(frame)
        guard visible.length > 0 else {
            fail("Quartz rendered no Markdown source text on a page; refusing an incomplete PDF")
        }

        guard let cgImage = bitmap.makeImage() else {
            fail("Quartz could not finalize a page bitmap")
        }
        let nsImage = NSImage(cgImage: cgImage, size: page.size)
        guard let pdfPage = PDFPage(image: nsImage) else {
            fail("PDFKit could not create a page from the Quartz rendering")
        }
        pdfPage.setBounds(page, for: .mediaBox)
        document.insert(pdfPage, at: document.pageCount)
        location += visible.length
    }

    if attributed.length == 0 {
        let blank = NSImage(size: page.size)
        guard let pdfPage = PDFPage(image: blank) else {
            fail("PDFKit could not create an empty Markdown page")
        }
        pdfPage.setBounds(page, for: .mediaBox)
        document.insert(pdfPage, at: 0)
    }

    guard document.write(to: temporaryURL) else {
        fail("PDFKit could not write the Quartz-rendered document")
    }
    guard FileManager.default.fileExists(atPath: temporaryURL.path) else {
        fail("PDFKit reported success but produced no PDF")
    }
    if FileManager.default.fileExists(atPath: outputURL.path) {
        try FileManager.default.removeItem(at: outputURL)
    }
    try FileManager.default.moveItem(at: temporaryURL, to: outputURL)

    print("title=\(title)")
    print("source_representation=verbatim Markdown, monospaced 9pt, rasterized at 216dpi")
    print("producer=AppKit NSFont + CoreText CTFramesetter + Quartz bitmap + PDFKit PDFDocument")
    print("creator=AppKit/Quartz CoreText raster pages + PDFKit")
    print("pages_written=\(document.pageCount)")
} catch {
    fail("Quartz Markdown PDF generation failed: \(error)")
}
