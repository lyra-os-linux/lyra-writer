#
# spec file for package lyra-writer
#
# Copyright (c) 2026 Rodrigo Brito
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#

Name:           lyra-writer
Version:        0.1.0
Release:        0
Summary:        Write Lyra OS installation media to a USB stick
License:        GPL-3.0-or-later
Group:          System/Management
URL:            https://github.com/lyra-os-linux/lyra-writer
Source0:        %{name}-%{version}.tar.zst
Source1:        vendor.tar.zst

BuildRequires:  cargo
BuildRequires:  cargo-packaging
BuildRequires:  rust >= 1.92
# Interface em Tauri 2 (WebKitGTK); os arquivos de ui/ são embutidos no binário.
BuildRequires:  pkgconfig(gtk+-3.0)
BuildRequires:  pkgconfig(webkit2gtk-4.1)
BuildRequires:  pkgconfig(javascriptcoregtk-4.1)
BuildRequires:  pkgconfig(libsoup-3.0)
BuildRequires:  pkgconfig
BuildRequires:  desktop-file-utils
BuildRequires:  appstream-glib
BuildRequires:  zstd
# Os pendrives são listados e abertos para escrita pelo udisks2, com
# autorização do polkit; o app não roda como root.
Requires:       udisks2

%description
O Lyra Writer grava a mídia de instalação do Lyra OS num pendrive e confere o
resultado, como o Fedora Media Writer. Só lista discos USB removíveis inteiros,
abre o dispositivo pelo udisks2 (Block.OpenDevice, autorizado pelo polkit) e
compara o SHA-256 do pendrive com o da imagem depois de gravar.

Prévia: a gravação de uma ISO local é real; o download das edições e a
restauração do pendrive ainda são simulados.

%prep
# -a1 desempacota o vendor.tar.zst (cargo_vendor) por cima do código; ele já
# traz o .cargo/config.toml que aponta o cargo para as crates vendorizadas.
%autosetup -a1

%build
%{cargo_build}

%install
install -Dm0755 target/release/lyra-writer %{buildroot}%{_bindir}/lyra-writer
install -Dm0644 data/org.lyraos.Writer.desktop \
    %{buildroot}%{_datadir}/applications/org.lyraos.Writer.desktop
install -Dm0644 data/org.lyraos.Writer.metainfo.xml \
    %{buildroot}%{_datadir}/metainfo/org.lyraos.Writer.metainfo.xml
install -Dm0644 data/icons/org.lyraos.Writer.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/org.lyraos.Writer.svg
install -Dm0644 data/icons/org.lyraos.Writer-symbolic.svg \
    %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/org.lyraos.Writer-symbolic.svg

desktop-file-validate %{buildroot}%{_datadir}/applications/org.lyraos.Writer.desktop
appstream-util validate-relax --nonet \
    %{buildroot}%{_datadir}/metainfo/org.lyraos.Writer.metainfo.xml

%check
# Filtro de pendrives e verificação SHA-256; o que precisa do barramento do
# sistema se pula sozinho no ambiente de build.
cargo test --offline --workspace

%post
%desktop_database_post
%icon_theme_cache_post

%postun
%desktop_database_postun
%icon_theme_cache_postun

%files
%license LICENSE
%doc README.md
%{_bindir}/lyra-writer
%{_datadir}/applications/org.lyraos.Writer.desktop
%{_datadir}/metainfo/org.lyraos.Writer.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/org.lyraos.Writer.svg
%{_datadir}/icons/hicolor/symbolic/apps/org.lyraos.Writer-symbolic.svg

%changelog
