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
    let markdown = try String(contentsOf: inputURL, encoding: .utf8)
    let parsing = NSAttributedString.MarkdownParsingOptions(interpretedSyntax: .full)
    let parsed = try NSAttributedString(
        markdown: markdown,
        options: parsing,
        baseURL: inputURL.deletingLastPathComponent()
    )
    let attributed = NSMutableAttributedString(attributedString: parsed)
    var missingFontRanges: [NSRange] = []
    attributed.enumerateAttribute(.font, in: NSRange(location: 0, length: attributed.length)) { value, range, _ in
        if value == nil {
            missingFontRanges.append(range)
        }
    }
    for range in missingFontRanges {
        attributed.addAttribute(.font, value: NSFont.systemFont(ofSize: 10), range: range)
    }

    let page = CGRect(x: 0, y: 0, width: 612, height: 792)
    let content = CGRect(x: 54, y: 54, width: 504, height: 684)
    let auxiliary: [CFString: Any] = [
        kCGPDFContextTitle: title,
        kCGPDFContextAuthor: "tzim fixed #57 measurement",
        kCGPDFContextCreator: "AppKit/Quartz native Markdown print-to-PDF",
    ]
    let storage = NSMutableData()
    guard let consumer = CGDataConsumer(data: storage as CFMutableData) else {
        fail("Quartz could not create a PDF data consumer")
    }
    var mediaBox = page
    guard let context = CGContext(
        consumer: consumer,
        mediaBox: &mediaBox,
        auxiliaryInfo: auxiliary as CFDictionary
    ) else {
        fail("Quartz could not create a PDF context")
    }

    let framesetter = CTFramesetterCreateWithAttributedString(attributed as CFAttributedString)
    var location = 0
    while location < attributed.length {
        context.beginPDFPage(nil)
        let path = CGPath(rect: content, transform: nil)
        let frame = CTFramesetterCreateFrame(
            framesetter,
            CFRange(location: location, length: 0),
            path,
            nil
        )
        CTFrameDraw(frame, context)
        let visible = CTFrameGetVisibleStringRange(frame)
        if visible.length <= 0 {
            context.endPDFPage()
            fail("Quartz rendered no text on a page; refusing an incomplete PDF")
        }
        location += visible.length
        context.endPDFPage()
    }
    if attributed.length == 0 {
        context.beginPDFPage(nil)
        context.endPDFPage()
    }
    context.closePDF()
    try storage.write(to: outputURL, options: .atomic)
    print("producer=AppKit NSAttributedString(markdown:) + Quartz CGContext PDFContext")
    print("creator=AppKit/Quartz native Markdown print-to-PDF")
    print("pages_written_with=CoreText CTFramesetter / Quartz PDFContext")
} catch {
    fail("Quartz Markdown rendering failed: \(error)")
}
