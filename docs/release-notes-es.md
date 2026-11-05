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

### Actualizaciones

A partir de esta versión **no hace falta volver a descargar el zip**: al arrancar, la app consulta el repositorio `MyOpenFadeR-Mac-Legacy` y, si hay una compilación más nueva, abre un aviso con **UPDATE**. Ese botón descarga el archivo, lo verifica con el SHA256 publicado y reemplaza solo `MyOpenFadeR.app`; después se reinicia sola. La carpeta de datos, el modelo de STEMS y todo lo demás no se tocan.

- El archivo lo escribe la propia app, así que no queda en cuarentena: no hay que hacer clic derecho > Abrir tras una actualización.
- Solo se ofrecen las publicaciones de este repositorio, que son las de Catalina e Intel.

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

El plugin se descarga su propio `yt-dlp` en `~/Library/Application Support/openfader/plugins/yt-dlp/bin`, que es una carpeta escribible: así puede auto-actualizarse sin tocar la app, que es de solo lectura y se reemplaza en cada actualización. La app comprueba y repara sola los permisos de ejecución de sus herramientas y de esos binarios al arrancar y antes de lanzar un plugin, así que no hay que ejecutar `chmod` a mano.

### Mapas MIDI: exportar e importar

En el panel **MIDI** (el chip `MIDI`, junto a `DEVICE` y `PREFS`) hay dos botones, debajo de la línea del controlador conectado:

- **EXPORT MAPPING** guarda en un único `.xml` el mapa que está usando el controlador, con los cambios que hayas hecho con MIDI LEARN ya incluidos (tu fila gana para el mismo control). Eliges dónde guardarlo y la carpeta se abre al terminar.
- **IMPORT MAPPING** lee un archivo exportado así (o cualquiera del formato de MyOpenFadeR), lo valida antes de tocar nada y lo instala en `~/Library/Application Support/openfader/mappings/` con el nombre del mapa en uso, de modo que lo reemplaza y el controlador se recarga solo. El resultado aparece en el mismo panel.

Sirve para llevar tu mapa y tus bindings a otro Mac, o para recuperarlos después de reinstalar.

### Avisos

- **Comparte la carpeta de datos** (`~/Library/Application Support/openfader`) con el MyOpenFadeR principal. Si abres este build y luego vuelves al principal, la base de datos puede quedar actualizada a un esquema más nuevo y este build la rechazará. Para volver atrás, borra o restaura `store.db`. Detalles en `LEGACY.md`.
- En el log pueden aparecer avisos de temas (`unknown token "studio-*"`, `unknown key "locales"`): son de los temas del repositorio actual, que pueden usar campos más nuevos que esta compilación. El tema carga igual.
- Si una salida de audio no arranca (por ejemplo una controladora o un HDMI sin pantalla conectado), la app lo reintenta y, si sigue fallando, sigue con la salida del sistema en vez de quedarse colgada. El motivo aparece en el log.
- Esta compilación se publica aparte del MyOpenFadeR principal, en el repositorio `MyOpenFadeR-Mac-Legacy`: las actualizaciones que ofrece son siempre las de Catalina e Intel.
