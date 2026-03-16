import 'dart:async';
import 'dart:js_interop';
import 'dart:typed_data';
import 'package:web/web.dart' as web;

class PickedFile {
  final String name;
  final Uint8List bytes;
  PickedFile({required this.name, required this.bytes});
}

/// Pick a file using the browser's native file input.
/// Returns null if the user cancels.
Future<PickedFile?> pickFile({String accept = '*'}) async {
  final completer = Completer<PickedFile?>();

  final input = web.document.createElement('input') as web.HTMLInputElement;
  input.type = 'file';
  input.accept = accept;
  input.click();

  input.addEventListener('change', (web.Event event) {
    final files = input.files;
    if (files == null || files.length == 0) {
      completer.complete(null);
      return;
    }
    final file = files.item(0)!;
    final reader = web.FileReader();
    reader.readAsArrayBuffer(file);
    reader.addEventListener('loadend', (web.Event e) {
      final result = reader.result;
      if (result != null) {
        final arrayBuffer = result as JSArrayBuffer;
        final bytes = arrayBuffer.toDart.asUint8List();
        completer.complete(PickedFile(name: file.name, bytes: bytes));
      } else {
        completer.complete(null);
      }
    }.toJS);
    reader.addEventListener('error', (web.Event e) {
      completer.complete(null);
    }.toJS);
  }.toJS);

  return completer.future;
}
