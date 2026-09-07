#!/bin/sh
set -eu
# Build-only script, shared by the Alpine API and Ubuntu LLM images.
work_dir="$1"
install_dir="$2"
jxl_source="$3"
mkdir -p "$work_dir"
cd "$work_dir"
curl -fsSL https://download.adobe.com/pub/adobe/dng/dng_sdk_1_7_1_2652_20260714.zip -o sdk.zip
echo '73499b47f4683e12120a234bd0946f02e52ab2ff9834bcbd0e9f8ab4f923360e  sdk.zip' | sha256sum -c -
unzip -q sdk.zip
case "$jxl_source" in
    bundled)
        cmake -S dng_sdk_1_7_1/libjxl/libjxl -B jxl-build -G Ninja \
            -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$install_dir" \
            -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_INSTALL_RPATH="$install_dir/lib" \
            -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DJPEGXL_FORCE_SYSTEM_BROTLI=ON \
            -DBUILD_TESTING=OFF -DJPEGXL_ENABLE_TOOLS=OFF -DJPEGXL_ENABLE_EXAMPLES=OFF \
            -DJPEGXL_ENABLE_BENCHMARK=OFF -DJPEGXL_ENABLE_MANPAGES=OFF \
            -DJPEGXL_ENABLE_JPEGLI=OFF -DJPEGXL_ENABLE_SJPEG=OFF
        cmake --build jxl-build --parallel 2
        cmake --install jxl-build
        ;;
    system) ;;
    *) echo "Expected system or bundled JPEG XL source" >&2; exit 2 ;;
esac
export PKG_CONFIG_PATH="$install_dir/lib/pkgconfig"
git clone https://github.com/ssh4net/DNG-CMake.git sdk-build
git -C sdk-build checkout 685f7139ba87afba2e40d4fd24a6455db6f8e9d9
cp -R dng_sdk_1_7_1/dng_sdk sdk-build/
cp -R dng_sdk_1_7_1/xmp sdk-build/
patch -d sdk-build -p1 < /decoder/dng-sdk-portability.patch
cmake -S sdk-build -B sdk-build/build -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DCMAKE_PREFIX_PATH="$install_dir" -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_INSTALL_PREFIX="$install_dir" -DDNG_WITH_XMP=ON \
    -DDNG_WITH_JXL=ON -DBUILD_DNG_VALIDATE=OFF -DDNG_THREAD_SAFE=OFF
cmake --build sdk-build/build --parallel 2
cmake --install sdk-build/build
git clone https://github.com/LibRaw/LibRaw.git libraw
git -C libraw checkout b93f6e45c194f5df9b02a43b1af9a54b4f41f33f
cd libraw
patch -p1 < /decoder/libraw-dng-host.patch
autoreconf -fi
CPPFLAGS="-DUSE_DNGSDK -DqLinux=1 -DqDNGUseXMP=1 -DqDNGUseLibJXL=1 -DqDNGUseLibJPEG=1 -DUNIX_ENV=1 -I$install_dir/include -I$work_dir/sdk-build/dng_sdk/source -I$work_dir/sdk-build/xmp/toolkit/public/include" \
    CXXFLAGS='-O2 -fPIC' \
    LDFLAGS="-Wl,-rpath,$install_dir/lib" \
    LIBS="$(PKG_CONFIG_PATH="$install_dir/lib/pkgconfig" pkg-config --static --libs dng_sdk)" \
    ./configure --prefix="$install_dir" --disable-openmp --disable-examples --enable-shared || {
        sed -n '1,200p' config.log >&2
        exit 1
    }
make -j2
make install
# dcraw_emu is used by the separate LLM container; enable the same SDK at runtime.
make bin/dcraw_emu
mkdir -p "$install_dir/bin"
./libtool --mode=install install -m 755 bin/dcraw_emu "$install_dir/bin/dcraw_emu"
mkdir -p "$install_dir/share/licenses"
cp "$work_dir/dng_sdk_1_7_1/LICENSE.txt" "$install_dir/share/licenses/Adobe-DNG-SDK.txt"
cp LICENSE.LGPL LICENSE.CDDL "$install_dir/share/licenses/"
cp "$work_dir/sdk-build/LICENSE" "$install_dir/share/licenses/DNG-CMake.txt"
