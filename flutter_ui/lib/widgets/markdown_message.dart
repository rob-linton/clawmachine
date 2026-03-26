import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:highlight/highlight.dart' show highlight;
import 'package:highlight/languages/all.dart' as all_languages;

/// Renders a chat message as markdown with syntax-highlighted code blocks.
class MarkdownMessage extends StatelessWidget {
  final String content;

  const MarkdownMessage({super.key, required this.content});

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

    return MarkdownBody(
      data: content,
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

    return _CodeBlockWidget(code: code, language: language);
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
