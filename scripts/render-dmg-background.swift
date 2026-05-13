import AppKit

let outputPath = CommandLine.arguments.dropFirst().first ?? "src-tauri/assets/dmg-background.png"
let backingScale = 2
let canvasWidth = 760
let canvasHeight = 560
let canvasSize = NSSize(width: canvasWidth, height: canvasHeight)

func color(_ hex: UInt32, alpha: CGFloat = 1) -> NSColor {
    let red = CGFloat((hex >> 16) & 0xff) / 255
    let green = CGFloat((hex >> 8) & 0xff) / 255
    let blue = CGFloat(hex & 0xff) / 255
    return NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha)
}

func topY(_ y: CGFloat, height: CGFloat) -> CGFloat {
    CGFloat(canvasHeight) - y - height
}

func drawTopCircle(centerX: CGFloat, centerY: CGFloat, radius: CGFloat, fill: NSColor) {
    fill.setFill()
    NSBezierPath(
        ovalIn: NSRect(
            x: centerX - radius,
            y: CGFloat(canvasHeight) - centerY - radius,
            width: radius * 2,
            height: radius * 2
        )
    ).fill()
}

func drawDotColumn(x: CGFloat, bottomY: CGFloat, height: Int, mirroredFromTop rowTop: CGFloat? = nil) {
    let dotCount = max(1, height / 8 + 1)
    let opacities: [CGFloat]

    switch height {
    case 4:
        opacities = [1]
    case 12:
        opacities = [0.7, 1]
    case 20:
        opacities = [0.7, 0.7, 1]
    case 28:
        opacities = [0.5, 0.7, 0.7, 1]
    case 36:
        opacities = [0.5, 0.5, 0.7, 0.7, 1]
    case 44:
        opacities = [0.3, 0.5, 0.5, 0.7, 0.7, 1]
    case 52:
        opacities = [0.3, 0.3, 0.5, 0.5, 0.7, 0.7, 1]
    default:
        opacities = Array(repeating: 1, count: dotCount)
    }

    for index in 0..<dotCount {
        let localY = CGFloat(2 + index * 8)
        let centerY: CGFloat

        if let rowTop {
            centerY = rowTop + CGFloat(height) - localY
        } else {
            centerY = bottomY - CGFloat(height) + localY
        }

        drawTopCircle(
            centerX: x + 2,
            centerY: centerY,
            radius: 2,
            fill: color(0x7d4af5, alpha: opacities[index])
        )
    }
}

func drawSystemFileIcon(path: String, centerX: CGFloat, centerY: CGFloat, size: CGFloat) {
    let icon = NSWorkspace.shared.icon(forFile: path)
    icon.size = NSSize(width: size, height: size)

    let rect = NSRect(
        x: centerX - size / 2,
        y: topY(centerY - size / 2, height: size),
        width: size,
        height: size
    )

    icon.draw(
        in: rect,
        from: NSRect(origin: .zero, size: icon.size),
        operation: .sourceOver,
        fraction: 1
    )
}

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: canvasWidth * backingScale,
    pixelsHigh: canvasHeight * backingScale,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
)!
rep.size = canvasSize

let graphicsContext = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = graphicsContext

color(0xedebf5).setFill()
NSBezierPath(rect: NSRect(origin: .zero, size: canvasSize)).fill()

if let gradient = CGGradient(
    colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
    colors: [
        color(0x7d4af5, alpha: 0.24).cgColor,
        color(0x7d4af5, alpha: 0.13).cgColor,
        color(0x7d4af5, alpha: 0).cgColor
    ] as CFArray,
    locations: [0, 0.42, 1]
) {
    let cgContext = graphicsContext.cgContext
    cgContext.saveGState()
    cgContext.translateBy(x: 0, y: CGFloat(canvasHeight))
    cgContext.scaleBy(x: 1, y: -1)
    cgContext.drawRadialGradient(
        gradient,
        startCenter: CGPoint(x: 380, y: 0),
        startRadius: 0,
        endCenter: CGPoint(x: 380, y: 0),
        endRadius: 390,
        options: [.drawsAfterEndLocation]
    )
    cgContext.restoreGState()
}

let titleLines = [
    "Drag the Parrot app icon into the",
    "Applications folder to install"
]
let titleFont = NSFont(name: "Roobert-Regular", size: 30)
    ?? NSFont(name: "Roobert", size: 30)
    ?? NSFont.systemFont(ofSize: 30, weight: .regular)
let paragraphStyle = NSMutableParagraphStyle()
paragraphStyle.alignment = .center
paragraphStyle.lineBreakMode = .byClipping
let titleAttributes: [NSAttributedString.Key: Any] = [
    .foregroundColor: color(0x1b1824),
    .font: titleFont,
    .kern: -0.3,
    .paragraphStyle: paragraphStyle
]

for (index, line) in titleLines.enumerated() {
    let attributedLine = NSAttributedString(string: line, attributes: titleAttributes)
    let lineSize = attributedLine.size()
    let lineX = (CGFloat(canvasWidth) - lineSize.width) / 2
    let lineY = topY(80 + CGFloat(index * 36), height: 36)
    attributedLine.draw(at: NSPoint(x: lineX, y: lineY))
}

let columnHeights = [
    28, 20, 36, 52, 28, 44, 36, 20, 20, 12, 4, 4, 12,
    28, 36, 28, 28, 12, 36, 52, 44, 28, 52, 36, 44, 52,
    52, 52, 36, 28, 20, 44, 28, 20, 4, 12, 20, 12, 12,
    4, 12, 28, 20, 36, 12, 4, 20, 28, 36, 28, 28, 44, 52
]

let waveX: CGFloat = 170
let topRowBottomY: CGFloat = 296
let bottomRowTopY: CGFloat = 292

for (index, height) in columnHeights.enumerated() {
    let x = waveX + CGFloat(index * 8)
    drawDotColumn(x: x, bottomY: topRowBottomY, height: height)
    drawDotColumn(x: x, bottomY: topRowBottomY, height: height, mirroredFromTop: bottomRowTopY)
}

// Finder draws the real /Applications alias above this background.
// This fallback makes the target visible when macOS fails to render the alias icon.
drawSystemFileIcon(
    path: "/Applications",
    centerX: 594,
    centerY: 292,
    size: 112
)

NSGraphicsContext.restoreGraphicsState()

let outputURL = URL(fileURLWithPath: outputPath)
let outputDirectory = outputURL.deletingLastPathComponent()
try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)

guard let data = rep.representation(using: .png, properties: [:]) else {
    fatalError("Could not encode DMG background PNG")
}

try data.write(to: outputURL)
print("Wrote \(outputPath)")
