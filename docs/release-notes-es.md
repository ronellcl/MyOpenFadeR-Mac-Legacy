**Compilación legacy para macOS 10.15 Catalina (Intel).** Misma aplicación que MyOpenFadeR 0.5.0, con **STEMS funcionando** en Catalina.

### Requisitos

- **macOS 10.15.7 Catalina, Intel (x86_64)**. Es el único sistema soportado.
- **No** funciona en Apple silicon ni en 10.13 / 10.14. Puede arrancar en Intel 11–15, pero ahí no está probado.
- Unos 8 GB de RAM recomendados (STEMS usa ~1,2 GB por pista; la app completa cabe en 4 GB).

### Instalación

1. Descarga `MyOpenFadeR-<versión>-macos-catalina-x86_64.zip` y descomprímelo.
2. Mueve `MyOpenFadeR.app` a **Aplicaciones**.
3. La app **no está firmada ni notarizada** (no hay cuenta de desarrollador). La primera vez:
   - **clic derecho > Abrir** y confirmar, o
   - en Terminal: `xattr -dr com.apple.quarantine /Applications/MyOpenFadeR.app`

   Si macOS dice que está *dañada*, es lo mismo: ejecuta el comando de arriba y vuelve a abrirla.

### Qué incluye

- La app, con **ONNX Runtime 1.16.3** dentro (`Contents/Frameworks`), que es la última versión cuyo binario de macOS Intel funciona en 10.15.
- `ffmpeg` y `ffprobe` estáticos, para leer y exportar audio y vídeo.
- 22 plugins nativos, 16 temas y 177 mappings de controladores, ya compilados para x86_64.
- Las licencias de ONNX Runtime y de ffmpeg, y `LEGACY.md` con todas las diferencias respecto al build principal.

### STEMS

Funciona con CPU. La primera vez descarga el modelo (316 MB) a `~/Library/Caches/openfader/models/htdemucs.onnx`, se verifica por SHA256 y luego queda en caché.

- En un Intel de 4 núcleos: **~3 minutos por minuto de audio** en calidad *Quality* (una pista de 4 min ≈ 11–12 min). En *Speed*, bastante menos.
- **No hay GPU**: ONNX Runtime 1.16 no tiene proveedor WebGPU. La app lo indicará y separará en CPU.
- Los stems se van reproduciendo a medida que se calculan; la pista suena completa mientras tanto.

### yt-dlp (YouTube y SoundCloud)

El plugin se descarga su propio `yt-dlp` en `~/Library/Application Support/openfader/plugins/yt-dlp/bin`, que es una carpeta escribible: así puede auto-actualizarse sin tocar la app, que es de solo lectura y se reemplaza en cada actualización. Si alguna vez ves `PermissionError: [Errno 13]` sobre ese binario, cierra la app y ejecuta:

```sh
chmod -R +x ~/Library/Application\ Support/openfader/plugins/yt-dlp/bin
```

### Avisos

- **Comparte la carpeta de datos** (`~/Library/Application Support/openfader`) con el MyOpenFadeR principal. Si abres este build y luego vuelves al principal, la base de datos puede quedar actualizada a un esquema más nuevo y este build la rechazará. Para volver atrás, borra o restaura `store.db`. Detalles en `LEGACY.md`.
- En el log pueden aparecer avisos de temas (`unknown token "studio-*"`, `unknown key "locales"`): son de los temas del repositorio actual, que pueden usar campos más nuevos que esta compilación. El tema carga igual.
- Puede decir que la actualización automática de la app no está disponible: este build se publica aparte del principal (repositorio `MyOpenFadeR-Mac-Legacy`).
