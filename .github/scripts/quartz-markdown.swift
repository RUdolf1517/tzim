import AppKit
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
    // Print the registered Markdown source verbatim. Keeping the source syntax
    // visible avoids dropping or fetching any image, link, or HTML content.
    let markdown = try String(contentsOf: inputURL, encoding: .utf8)
    let font = NSFont.monospacedSystemFont(ofSize: 9, weight: .regular)
    let attributed = NSAttributedString(string: markdown, attributes: [.font: font])

    let page = CGRect(x: 0, y: 0, width: 612, height: 792)
    let content = CGRect(x: 54, y: 54, width: 504, height: 684)
    let temporaryURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("tzim-fixed-quartz-\(UUID().uuidString).pdf")

    let application = NSApplication.shared
    _ = application.setActivationPolicy(.prohibited)

    let textView = NSTextView(
        frame: NSRect(x: 0, y: 0, width: content.width, height: content.height)
    )
    textView.isEditable = false
    textView.isSelectable = false
    textView.drawsBackground = false
    textView.isHorizontallyResizable = false
    textView.isVerticallyResizable = true
    textView.textContainerInset = .zero
    textView.textContainer?.containerSize = NSSize(
        width: content.width,
        height: CGFloat.greatestFiniteMagnitude
    )
    textView.textContainer?.widthTracksTextView = false
    textView.textStorage?.setAttributedString(attributed)

    guard let textContainer = textView.textContainer,
          let layoutManager = textView.layoutManager else {
        fail("AppKit could not create a Markdown source text layout")
    }
    layoutManager.ensureLayout(for: textContainer)
    let usedRect = layoutManager.usedRect(for: textContainer)
    let documentHeight = max(content.height, ceil(usedRect.maxY + 12))
    textView.setFrameSize(NSSize(width: content.width, height: documentHeight))

    let printOptions: [NSPrintInfo.AttributeKey: Any] = [
        .jobDisposition: NSPrintInfo.JobDisposition.save,
        .jobSavingURL: temporaryURL,
    ]
    let printInfo = NSPrintInfo(dictionary: printOptions)
    printInfo.paperSize = page.size
    printInfo.leftMargin = content.minX
    printInfo.rightMargin = page.width - content.maxX
    printInfo.topMargin = content.minY
    printInfo.bottomMargin = page.height - content.maxY
    printInfo.horizontalPagination = .fit
    printInfo.verticalPagination = .automatic
    printInfo.isHorizontallyCentered = false
    printInfo.isVerticallyCentered = false

    let operation = NSPrintOperation(view: textView, printInfo: printInfo)
    operation.showsPrintPanel = false
    operation.showsProgressPanel = false
    guard operation.run() else {
        fail("AppKit/Quartz print operation did not produce a PDF")
    }

    guard FileManager.default.fileExists(atPath: temporaryURL.path) else {
        fail("AppKit/Quartz print operation returned success but produced no PDF")
    }
    if FileManager.default.fileExists(atPath: outputURL.path) {
        try FileManager.default.removeItem(at: outputURL)
    }
    try FileManager.default.moveItem(at: temporaryURL, to: outputURL)

    print("title=\(title)")
    print("source_representation=verbatim Markdown, monospaced 9pt")
    print("producer=AppKit NSTextView + NSPrintOperation with native Quartz PDF output")
    print("creator=AppKit/Quartz native Markdown source print-to-PDF")
    print("pages_written_with=AppKit NSTextView pagination")
} catch {
    fail("Quartz Markdown rendering failed: \(error)")
}
