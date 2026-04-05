# ftxui_bindings.nim — Nim {.importcpp.} bindings for FTXUI
#
# Single source of truth for FTXUI interop.
# Nim calls C++ directly — no wrapper layer.

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

proc canvasElement*(width, height: cint, fn: proc(c: var Canvas) {.cdecl.}): Element =
  ## Wraps a canvas drawing function into an Element.
  ## Note: FTXUI's canvas() takes std::function<void(Canvas&)>.
  ## We use a C++ lambda wrapper via emit.
  discard  # placeholder — need emit approach

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
