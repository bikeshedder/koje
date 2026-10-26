# koje's Hyprland plugin (ADR 0051). Built in the package's Ubuntu container
# (build/Containerfile) by scripts/build-deb.sh; `make` there writes $(OUT).
OUT ?= koje-hyprland.so
SRC = $(wildcard src/*.cpp)

$(OUT): $(SRC) $(wildcard src/*.hpp)
	$(CXX) -shared -fPIC --no-gnu-unique -O2 -g -std=c++2b -Wall -Wno-narrowing \
		$(SRC) -o $(OUT) \
		`pkg-config --cflags pixman-1 libdrm hyprland pangocairo libinput libudev wayland-server xkbcommon`
