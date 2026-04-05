.PHONY: all ftxui nim clean

FTXUI_BUILD_DIR = src/ftxui/build
FTXUI_LIB = $(FTXUI_BUILD_DIR)/libftxui_dashboard.a

all: ftxui nim

ftxui: $(FTXUI_LIB)

$(FTXUI_LIB): src/ftxui/CMakeLists.txt src/ftxui/ftxui_dashboard.cpp src/ftxui/ftxui_dashboard.h
	cmake -B $(FTXUI_BUILD_DIR) -S src/ftxui -DCMAKE_BUILD_TYPE=Release
	cmake --build $(FTXUI_BUILD_DIR) -j$$(sysctl -n hw.ncpu)

nim: $(FTXUI_LIB)
	nim c -d:ftxui src/main.nim

nim-fallback:
	nim c src/main.nim

clean:
	rm -rf $(FTXUI_BUILD_DIR) src/main
