.PHONY: all ftxui nim clean

FTXUI_BUILD_DIR = src/ftxui/build
FTXUI_DOM_LIB = $(FTXUI_BUILD_DIR)/_deps/ftxui-build/libftxui-dom.a

all: ftxui nim

ftxui: $(FTXUI_DOM_LIB)

$(FTXUI_DOM_LIB): src/ftxui/CMakeLists.txt
	cmake -B $(FTXUI_BUILD_DIR) -S src/ftxui -DCMAKE_BUILD_TYPE=Release
	cmake --build $(FTXUI_BUILD_DIR) -j$$(sysctl -n hw.ncpu)

nim: $(FTXUI_DOM_LIB)
	nim cpp -d:ftxui src/main.nim

nim-fallback:
	nim c src/main.nim

clean:
	rm -rf $(FTXUI_BUILD_DIR) src/main
