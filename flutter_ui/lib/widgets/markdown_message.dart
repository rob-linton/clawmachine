import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'dart:ui_web' as ui_web;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:highlight/highlight.dart' show highlight;
import 'package:highlight/languages/all.dart' as all_languages;
import 'package:web/web.dart' as web;

/// Renders a chat message as markdown with syntax-highlighted code blocks.
class MarkdownMessage extends StatelessWidget {
  final String content;

  const MarkdownMessage({super.key, required this.content});

  /// Adds two trailing spaces to lines so Markdown renders hard line breaks.
  /// Skips blank lines (paragraph separators) and code fence regions.
  static String _preserveLineBreaks(String text) {
    final lines = text.split('\n');
    final buf = StringBuffer();
    bool inCodeFence = false;
    for (int i = 0; i < lines.length; i++) {
      final line = lines[i];
      if (line.trimLeft().startsWith('```')) inCodeFence = !inCodeFence;
      if (i < lines.length - 1 && !inCodeFence && line.isNotEmpty && !line.endsWith('  ')) {
        buf.writeln('$line  ');
      } else {
        if (i < lines.length - 1) {
          buf.writeln(line);
        } else {
          buf.write(line);
        }
      }
    }
    return buf.toString();
  }

  static bool _languagesRegistered = false;
  static void _ensureLanguages() {
    if (!_languagesRegistered) {
      all_languages.allLanguages.forEach((name, mode) {
        highlight.registerLanguage(name, mode);
      });
      _languagesRegistered = true;
    }
  }

  @override
  Widget build(BuildContext context) {
    _ensureLanguages();

    // Convert single newlines to hard line breaks (two trailing spaces)
    // so that text like haikus preserves its line structure. Skip lines
    // that are already blank (paragraph breaks) or inside code fences.
    final processed = _preserveLineBreaks(content);

    return MarkdownBody(
      data: processed,
      selectable: true,
      styleSheet: MarkdownStyleSheet(
        p: const TextStyle(height: 1.5, fontSize: 14),
        code: TextStyle(
          fontFamily: 'monospace',
          fontSize: 13,
          backgroundColor: Colors.grey.shade900,
          color: Colors.grey.shade300,
        ),
        codeblockDecoration: BoxDecoration(
          color: const Color(0xFF1E1E1E),
          borderRadius: BorderRadius.circular(8),
        ),
        codeblockPadding: const EdgeInsets.all(12),
        blockquoteDecoration: BoxDecoration(
          border: Border(left: BorderSide(color: Colors.grey.shade600, width: 3)),
        ),
        blockquotePadding: const EdgeInsets.only(left: 12),
        tableBorder: TableBorder.all(color: Colors.grey.shade700, width: 0.5),
        tableHead: const TextStyle(fontWeight: FontWeight.bold),
        tableCellsPadding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        h1: const TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
        h2: const TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        h3: const TextStyle(fontSize: 16, fontWeight: FontWeight.bold),
      ),
      builders: {
        'code': _CodeBlockBuilder(),
      },
    );
  }
}

/// Custom code block builder with copy button and syntax highlighting.
class _CodeBlockBuilder extends MarkdownElementBuilder {
  @override
  Widget? visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    // Only handle code blocks (not inline code)
    if (element.tag != 'code') return null;
    final parent = element.attributes['class'];
    if (parent == null && !element.textContent.contains('\n')) return null;

    final language = parent?.replaceFirst('language-', '') ?? '';
    final code = element.textContent.trimRight();

    if (language == 'mermaid') {
      return _MermaidBlock(source: code);
    }

    return _CodeBlockWidget(code: code, language: language);
  }
}

/// Renders a Mermaid diagram by handing the source to mermaid.js (loaded
/// from a CDN in web/index.html) and embedding the resulting SVG via an
/// HtmlElementView. Each instance gets a unique view type so multiple
/// diagrams in the same message render independently.
class _MermaidBlock extends StatefulWidget {
  final String source;
  const _MermaidBlock({required this.source});

  @override
  State<_MermaidBlock> createState() => _MermaidBlockState();
}

class _MermaidBlockState extends State<_MermaidBlock> {
  static int _nextId = 0;
  late final String _viewType;
  late final web.HTMLDivElement _container;

  @override
  void initState() {
    super.initState();
    final id = _nextId++;
    _viewType = 'mermaid-block-$id';
    _container = web.HTMLDivElement()
      ..style.width = '100%'
      ..style.minHeight = '40px'
      ..style.background = '#1E1E1E'
      ..style.padding = '12px'
      ..style.borderRadius = '8px'
      ..style.color = '#ddd'
      ..style.fontFamily = 'monospace'
      ..style.fontSize = '12px'
      ..innerHTML = 'Rendering diagram...'.toJS as JSAny;
    ui_web.platformViewRegistry
        .registerViewFactory(_viewType, (int _) => _container);
    _renderWhenReady();
  }

  void _renderWhenReady() {
    final win = web.window as JSObject;
    if (win.getProperty<JSBoolean?>('mermaidReady'.toJS)?.toDart == true) {
      _render();
    } else {
      // mermaid.js still loading from CDN — wait for the ready event.
      web.window.addEventListener(
        'mermaid-ready',
        ((web.Event _) {
          _render();
        }).toJS,
      );
    }
  }

  Future<void> _render() async {
    try {
      final win = web.window as JSObject;
      final mermaid = win.getProperty<JSObject?>('mermaid'.toJS);
      if (mermaid == null) {
        _showError('mermaid.js not loaded');
        return;
      }
      final renderId = 'mermaid-svg-${DateTime.now().microsecondsSinceEpoch}';
      final result = await mermaid
          .callMethod<JSPromise>(
            'render'.toJS,
            renderId.toJS,
            widget.source.toJS,
          )
          .toDart;
      final svg = (result as JSObject)
          .getProperty<JSString>('svg'.toJS)
          .toDart;
      _container.innerHTML = svg.toJS as JSAny;
    } catch (e) {
      _showError('$e');
    }
  }

  void _showError(String msg) {
    if (!mounted) return;
    _container.innerHTML =
        ('<pre style="color:#f88;white-space:pre-wrap;">Mermaid render failed: $msg\n\n${_escape(widget.source)}</pre>')
            .toJS as JSAny;
  }

  String _escape(String s) => s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;');

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 8),
      constraints: const BoxConstraints(minHeight: 40),
      child: HtmlElementView(viewType: _viewType),
    );
  }
}

class _CodeBlockWidget extends StatelessWidget {
  final String code;
  final String language;

  const _CodeBlockWidget({required this.code, required this.language});

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 8),
      decoration: BoxDecoration(
        color: const Color(0xFF1E1E1E),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Header with language + copy button
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            decoration: BoxDecoration(
              color: Colors.grey.shade800,
              borderRadius: const BorderRadius.vertical(top: Radius.circular(8)),
            ),
            child: Row(
              children: [
                if (language.isNotEmpty)
                  Text(language, style: TextStyle(fontSize: 12, color: Colors.grey.shade400)),
                const Spacer(),
                InkWell(
                  onTap: () {
                    Clipboard.setData(ClipboardData(text: code));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Code copied'), duration: Duration(seconds: 1)),
                    );
                  },
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.copy, size: 14, color: Colors.grey.shade400),
                      const SizedBox(width: 4),
                      Text('Copy', style: TextStyle(fontSize: 12, color: Colors.grey.shade400)),
                    ],
                  ),
                ),
              ],
            ),
          ),
          // Code content
          Padding(
            padding: const EdgeInsets.all(12),
            child: SelectableText(
              code,
              style: const TextStyle(
                fontFamily: 'monospace',
                fontSize: 13,
                height: 1.4,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
