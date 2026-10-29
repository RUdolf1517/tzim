import AppKit
import CoreGraphics
import CoreText
import Darwin
import Foundation

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
    let temporaryURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("tzim-fixed-quartz-\(UUID().uuidString).pdf")
    let auxiliary: [CFString: Any] = [
        kCGPDFContextTitle: title,
        kCGPDFContextAuthor: "tzim fixed #57 measurement",
        kCGPDFContextCreator: "AppKit/Quartz CoreText Markdown source PDF",
    ]

    var pagesWritten = 0
    do {
        var mediaBox = page
        guard let context = CGContext(
            temporaryURL as CFURL,
            mediaBox: &mediaBox,
            auxiliary as CFDictionary
        ) else {
            fail("Quartz could not create a PDF context")
        }

        var location = 0
        while location < attributed.length {
            context.beginPDFPage(nil)
            context.saveGState()
            context.translateBy(x: 0, y: page.height)
            context.scaleBy(x: 1, y: -1)
            context.textMatrix = .identity

            let path = CGPath(rect: content, transform: nil)
            let frame = CTFramesetterCreateFrame(
                framesetter,
                CFRange(location: location, length: 0),
                path,
                nil
            )
            CTFrameDraw(frame, context)
            let visible = CTFrameGetVisibleStringRange(frame)
            context.restoreGState()
            context.endPDFPage()

            guard visible.length > 0 else {
                fail("Quartz rendered no Markdown source text on a page; refusing an incomplete PDF")
            }
            location += visible.length
            pagesWritten += 1
        }
        if attributed.length == 0 {
            context.beginPDFPage(nil)
            context.endPDFPage()
            pagesWritten = 1
        }
        context.closePDF()
    }

    guard FileManager.default.fileExists(atPath: temporaryURL.path) else {
        fail("Quartz PDF context returned success but produced no PDF")
    }
    if FileManager.default.fileExists(atPath: outputURL.path) {
        try FileManager.default.removeItem(at: outputURL)
    }
    try FileManager.default.moveItem(at: temporaryURL, to: outputURL)

    print("title=\(title)")
    print("source_representation=verbatim Markdown, monospaced 9pt")
    print("producer=AppKit NSFont + CoreText CTFramesetter + native Quartz PDFContext")
    print("creator=AppKit/Quartz CoreText Markdown source PDF")
    print("pages_written=\(pagesWritten)")
} catch {
    fail("Quartz Markdown PDF generation failed: \(error)")
}
