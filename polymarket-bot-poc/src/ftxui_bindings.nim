# ftxui_bindings.nim — Nim {.importcpp.} bindings for FTXUI
#
# Single source of truth for FTXUI interop.
# Nim calls C++ directly — no wrapper layer.

{.emit: """/*INCLUDESECTION*/
#include <memory>
#include <vector>
#include <algorithm>
""".}

{.passC: "-Isrc/ftxui/build/_deps/ftxui-src/include -std=c++17".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-dom.a".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-screen.a".}
{.passL: "src/ftxui/build/_deps/ftxui-build/libftxui-component.a".}
{.passL: "-lc++".}

# ── Element (shared_ptr<Node>) ──

type
  Element* {.importcpp: "ftxui::Element", header: "ftxui/dom/elements.hpp".} = object
  Elements* {.importcpp: "ftxui::Elements", header: "ftxui/dom/elements.hpp".} = object
  Decorator* {.importcpp: "ftxui::Decorator", header: "ftxui/dom/elements.hpp".} = object
  Canvas* {.importcpp: "ftxui::Canvas", header: "ftxui/dom/canvas.hpp".} = object
  FtxuiScreen* {.importcpp: "ftxui::Screen", header: "ftxui/screen/screen.hpp".} = object
  Dimensions* {.importcpp: "ftxui::Dimensions", header: "ftxui/screen/screen.hpp".} = object
  FtxuiColor* {.importcpp: "ftxui::Color", header: "ftxui/screen/color.hpp".} = object

  WidthOrHeight* {.importcpp: "ftxui::WidthOrHeight", header: "ftxui/dom/elements.hpp".} = enum
    WIDTH = 0, HEIGHT = 1

  Constraint* {.importcpp: "ftxui::Constraint", header: "ftxui/dom/elements.hpp".} = enum
    LESS_THAN = 0, EQUAL = 1, GREATER_THAN = 2

# ── Colors ──

proc colorDefault*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Default)", header: "ftxui/screen/color.hpp".}
proc colorRed*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Red)", header: "ftxui/screen/color.hpp".}
proc colorGreen*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Green)", header: "ftxui/screen/color.hpp".}
proc colorYellow*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Yellow)", header: "ftxui/screen/color.hpp".}
proc colorBlue*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Blue)", header: "ftxui/screen/color.hpp".}
proc colorCyan*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::Cyan)", header: "ftxui/screen/color.hpp".}
proc colorWhite*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::White)", header: "ftxui/screen/color.hpp".}
proc colorGrayDark*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::GrayDark)", header: "ftxui/screen/color.hpp".}
proc colorGrayLight*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::GrayLight)", header: "ftxui/screen/color.hpp".}
proc colorRedLight*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::RedLight)", header: "ftxui/screen/color.hpp".}
proc colorGreenLight*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::GreenLight)", header: "ftxui/screen/color.hpp".}
proc colorYellowLight*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::YellowLight)", header: "ftxui/screen/color.hpp".}
proc colorBlueLight*(): FtxuiColor {.importcpp: "ftxui::Color(ftxui::Color::BlueLight)", header: "ftxui/screen/color.hpp".}
proc colorRGB*(r, g, b: uint8): FtxuiColor {.importcpp: "ftxui::Color(#, #, #)", header: "ftxui/screen/color.hpp".}

# ── Elements creation ──

proc text*(s: cstring): Element {.importcpp: "ftxui::text(std::string(#))", header: "ftxui/dom/elements.hpp".}
proc text*(s: string): Element = text(s.cstring)

proc separator*(): Element {.importcpp: "ftxui::separator()", header: "ftxui/dom/elements.hpp".}
proc separatorLight*(): Element {.importcpp: "ftxui::separatorLight()", header: "ftxui/dom/elements.hpp".}
proc filler*(): Element {.importcpp: "ftxui::filler()", header: "ftxui/dom/elements.hpp".}
proc gauge*(progress: cfloat): Element {.importcpp: "ftxui::gauge(#)", header: "ftxui/dom/elements.hpp".}
proc emptyElement*(): Element {.importcpp: "ftxui::emptyElement()", header: "ftxui/dom/elements.hpp".}

# ── Container elements ──

proc initElements*(): Elements {.importcpp: "ftxui::Elements()", header: "ftxui/dom/elements.hpp".}
proc add*(e: var Elements, el: Element) {.importcpp: "#.push_back(#)".}

proc hbox*(elems: Elements): Element {.importcpp: "ftxui::hbox(#)", header: "ftxui/dom/elements.hpp".}
proc vbox*(elems: Elements): Element {.importcpp: "ftxui::vbox(#)", header: "ftxui/dom/elements.hpp".}

# ── Decorators ──

proc bold*(e: Element): Element {.importcpp: "ftxui::bold(#)", header: "ftxui/dom/elements.hpp".}
proc dim*(e: Element): Element {.importcpp: "ftxui::dim(#)", header: "ftxui/dom/elements.hpp".}
proc inverted*(e: Element): Element {.importcpp: "ftxui::inverted(#)", header: "ftxui/dom/elements.hpp".}
proc flex*(e: Element): Element {.importcpp: "ftxui::flex(#)", header: "ftxui/dom/elements.hpp".}
proc flexGrow*(e: Element): Element {.importcpp: "ftxui::flex_grow(#)", header: "ftxui/dom/elements.hpp".}
proc border*(e: Element): Element {.importcpp: "ftxui::border(#)", header: "ftxui/dom/elements.hpp".}
proc borderLight*(e: Element): Element {.importcpp: "ftxui::borderLight(#)", header: "ftxui/dom/elements.hpp".}
proc yframe*(e: Element): Element {.importcpp: "ftxui::yframe(#)", header: "ftxui/dom/elements.hpp".}
proc hcenter*(e: Element): Element {.importcpp: "ftxui::hcenter(#)", header: "ftxui/dom/elements.hpp".}
proc center*(e: Element): Element {.importcpp: "ftxui::center(#)", header: "ftxui/dom/elements.hpp".}

# Decorator application via |
proc applyDecorator*(e: Element, d: Decorator): Element {.importcpp: "(# | #)", header: "ftxui/dom/elements.hpp".}

# Color/bgcolor decorators
proc color*(c: FtxuiColor): Decorator {.importcpp: "ftxui::color(#)", header: "ftxui/dom/elements.hpp".}
proc bgcolor*(c: FtxuiColor): Decorator {.importcpp: "ftxui::bgcolor(#)", header: "ftxui/dom/elements.hpp".}

# Size decorator
proc size*(wh: WidthOrHeight, c: Constraint, value: cint): Decorator {.importcpp: "ftxui::size(#, #, #)", header: "ftxui/dom/elements.hpp".}

# Convenience: apply color to element
proc withColor*(e: Element, c: FtxuiColor): Element = applyDecorator(e, color(c))
proc withBgColor*(e: Element, c: FtxuiColor): Element = applyDecorator(e, bgcolor(c))
proc withSize*(e: Element, wh: WidthOrHeight, c: Constraint, value: int): Element =
  applyDecorator(e, size(wh, c, value.cint))

# ── Canvas ──

proc initCanvas*(width, height: cint): Canvas {.importcpp: "ftxui::Canvas(@)", header: "ftxui/dom/canvas.hpp".}
proc width*(c: Canvas): cint {.importcpp: "#.width()".}
proc height*(c: Canvas): cint {.importcpp: "#.height()".}

proc drawPointLine*(c: var Canvas, x1, y1, x2, y2: cint) {.importcpp: "#.DrawPointLine(#, #, #, #)".}
proc drawPointLine*(c: var Canvas, x1, y1, x2, y2: cint, col: FtxuiColor) {.importcpp: "#.DrawPointLine(#, #, #, #, #)".}
proc drawPoint*(c: var Canvas, x, y: cint, value: bool, col: FtxuiColor) {.importcpp: "#.DrawPoint(#, #, #, #)".}
proc drawBlockLine*(c: var Canvas, x1, y1, x2, y2: cint) {.importcpp: "#.DrawBlockLine(#, #, #, #)".}
proc drawBlockLine*(c: var Canvas, x1, y1, x2, y2: cint, col: FtxuiColor) {.importcpp: "#.DrawBlockLine(#, #, #, #, #)".}

proc canvasToElement*(c: Canvas): Element =
  ## Convert a pre-drawn Canvas to an Element.
  ## ConstRef<Canvas> is implicitly constructible from Canvas by value.
  var res: Element
  {.emit: [res, " = ftxui::canvas(std::move(", c, "));"].}
  res

proc makeLineChart*(data: ptr float32, count: cint, w, h: cint, col: FtxuiColor): Element =
  ## Create a line chart Element from float data array.
  ## Uses braille resolution (2x per char width, 4x per char height).
  let cw = w * 2  # braille: 2 dots per character horizontally
  let ch = h * 4  # braille: 4 dots per character vertically
  var c = initCanvas(cw, ch)
  if count < 2 or cw <= 0 or ch <= 0:
    return canvasToElement(c)
  let arr = cast[ptr UncheckedArray[float32]](data)
  # Find min/max with margin
  var minV = arr[0]
  var maxV = arr[0]
  for i in 1..<count:
    if arr[i] < minV: minV = arr[i]
    if arr[i] > maxV: maxV = arr[i]
  # Add 5% margin
  let margin = max((maxV - minV) * 0.05'f32, 0.001'f32)
  minV -= margin
  maxV += margin
  let rangeV = maxV - minV
  # Grid lines at 25%, 50%, 75%
  for pct in [0.25'f32, 0.50, 0.75]:
    let y = ch - cint(pct * ch.float32)
    var x: cint = 0
    while x < cw:
      c.drawPoint(x, y, true, colorGrayDark())
      x += 4
  # Map data to screen coordinates
  proc toY(v: float32): cint =
    ch - 1 - cint((v - minV) / rangeV * (ch - 2).float32)
  proc toX(i: int): cint =
    cint(i.float32 / (count - 1).float32 * (cw - 1).float32)
  # Draw smooth line using braille points only
  for i in 1..<count:
    c.drawPointLine(toX(i - 1), toY(arr[i - 1]), toX(i), toY(arr[i]), col)
  # Fill under the curve with braille dots (subtle, every other pixel)
  let dimGreen = colorRGB(0, 40, 15)
  for i in 0..<count:
    let x = toX(i)
    let y = toY(arr[i])
    var fy = y + 2
    while fy < ch:
      c.drawPoint(x, fy, true, dimGreen)
      fy += 2
  canvasToElement(c)

proc makeBarChart*(values: ptr int64, count: cint, w, h: cint, colors: ptr FtxuiColor): Element =
  ## Create a bar chart Element from int64 data.
  var c = initCanvas(w * 2, h * 2)  # block resolution
  let cw = c.width
  let ch = c.height
  if count <= 0 or cw <= 0 or ch <= 0:
    return canvasToElement(c)
  var maxV: int64 = 1
  for i in 0..<count:
    let v = cast[ptr UncheckedArray[int64]](values)[i]
    if v > maxV: maxV = v
  let barW = max(1, cw div count)
  for i in 0..<count:
    let v = cast[ptr UncheckedArray[int64]](values)[i]
    let bh = cint(v.float / maxV.float * ch.float)
    let x0 = cint(i) * barW
    let col = cast[ptr UncheckedArray[FtxuiColor]](colors)[i]
    for y in (ch - bh)..<ch:
      c.drawBlockLine(x0, y, x0 + barW - 1, y, col)
  canvasToElement(c)

proc makeDepthChart*(bidSizes, askSizes: ptr float64, bidCount, askCount: cint, w, h: cint): Element =
  ## Horizontal depth bars: bids green left, asks red right.
  var c = initCanvas(w * 2, h * 2)
  let cw = c.width
  let ch = c.height
  if cw <= 0 or ch <= 0: return canvasToElement(c)
  let cx = cw div 2
  var maxSize: float64 = 1
  for i in 0..<bidCount:
    let s = cast[ptr UncheckedArray[float64]](bidSizes)[i]
    if s > maxSize: maxSize = s
  for i in 0..<askCount:
    let s = cast[ptr UncheckedArray[float64]](askSizes)[i]
    if s > maxSize: maxSize = s
  let levels = max(bidCount, askCount)
  let barH = max(1, ch div (levels * 2 + 1))
  for i in 0..<bidCount:
    let s = cast[ptr UncheckedArray[float64]](bidSizes)[i]
    let bw = cint(s / maxSize * cx.float)
    let y = cint(i) * barH * 2
    if y < ch:
      c.drawBlockLine(cx - bw, y, cx - 1, y, colorGreen())
  for i in 0..<askCount:
    let s = cast[ptr UncheckedArray[float64]](askSizes)[i]
    let aw = cint(s / maxSize * cx.float)
    let y = cint(i) * barH * 2
    if y < ch:
      c.drawBlockLine(cx, y, cx + aw, y, colorRed())
  canvasToElement(c)

proc makeSparklineGraph*(data: ptr int16, count: cint, col: FtxuiColor): Element =
  ## Create a graph element from sparkline data.
  ## Uses {.emit.} to bridge to FTXUI's graph() with std::function.
  var res: Element
  {.emit: """
  auto dataPtr = (NI16*)`data`;
  int dataCount = `count`;
  auto graphColor = `col`;
  `res` = ftxui::graph([dataPtr, dataCount](int width, int height) -> std::vector<int> {
    std::vector<int> out(width, 0);
    int16_t maxVal = 1;
    for (int i = 0; i < std::min(width, dataCount); i++)
      if (dataPtr[i] > maxVal) maxVal = dataPtr[i];
    for (int i = 0; i < std::min(width, dataCount); i++)
      out[i] = std::min(static_cast<int>(dataPtr[i] * height / maxVal), height);
    return out;
  }) | ftxui::color(graphColor);
  """.}
  res

# ── Screen ──

proc screenCreate*(dims: Dimensions): FtxuiScreen {.importcpp: "ftxui::Screen::Create(#)", header: "ftxui/screen/screen.hpp".}
proc screenCreate*(w, h: Dimensions): FtxuiScreen {.importcpp: "ftxui::Screen::Create(#, #)", header: "ftxui/screen/screen.hpp".}
proc print*(s: FtxuiScreen) {.importcpp: "#.Print()".}
proc resetPositionStr*(s: FtxuiScreen, clear: bool = false): string =
  var res: cstring
  {.emit: [res, " = (char*)", s, ".ResetPosition(", clear, ").c_str();"].}
  $res

proc dimensionFull*(): Dimensions {.importcpp: "ftxui::Dimension::Full()", header: "ftxui/screen/screen.hpp".}
proc dimensionFit*(e: Element): Dimensions {.importcpp: "ftxui::Dimension::Fit(#)", header: "ftxui/screen/screen.hpp".}
proc dimensionFixed*(v: cint): Dimensions {.importcpp: "ftxui::Dimension::Fixed(#)", header: "ftxui/screen/screen.hpp".}

proc render*(screen: var FtxuiScreen, doc: Element) {.importcpp: "ftxui::Render(#, #)", header: "ftxui/dom/node.hpp".}
