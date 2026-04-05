.PHONY: all ftxui nim clean

FTXUI_BUILD_DIR = src/ftxui/build
FTXUI_LIBS = $(FTXUI_BUILD_DIR)/_deps/ftxui-build/libftxui-dom.a \
             $(FTXUI_BUILD_DIR)/_deps/ftxui-build/libftxui-screen.a \
             $(FTXUI_BUILD_DIR)/_deps/ftxui-build/libftxui-component.a

NPROC := $(shell nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

all: ftxui nim

ftxui: $(FTXUI_LIBS)

$(FTXUI_LIBS): src/ftxui/CMakeLists.txt
	cmake -B $(FTXUI_BUILD_DIR) -S src/ftxui -DCMAKE_BUILD_TYPE=Release
	cmake --build $(FTXUI_BUILD_DIR) -j$(NPROC)

nim: $(FTXUI_LIBS)
	nim cpp -d:ftxui src/main.nim

nim-fallback:
	nim c src/main.nim

clean:
	rm -rf $(FTXUI_BUILD_DIR) src/main
