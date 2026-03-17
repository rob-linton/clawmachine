class FileTreeNode {
  final String name;
  final String fullPath;
  final bool isDir;
  final int size;
  final List<FileTreeNode> children;
  bool expanded;

  FileTreeNode({
    required this.name,
    required this.fullPath,
    required this.isDir,
    required this.size,
    List<FileTreeNode>? children,
    this.expanded = false,
  }) : children = children ?? [];

  /// Build a tree from the flat file list returned by the API.
  static List<FileTreeNode> buildTree(List<dynamic> flatFiles) {
    // Map from directory path → node, for fast lookup during insertion
    final Map<String, FileTreeNode> dirMap = {};
    final List<FileTreeNode> roots = [];

    // Sort so directories come first, then alphabetically
    final sorted = List<Map<String, dynamic>>.from(
      flatFiles.map((f) => f as Map<String, dynamic>),
    )..sort((a, b) {
        final aDir = (a['is_dir'] as bool? ?? false) ? 0 : 1;
        final bDir = (b['is_dir'] as bool? ?? false) ? 0 : 1;
        if (aDir != bDir) return aDir.compareTo(bDir);
        return (a['path'] as String).compareTo(b['path'] as String);
      });

    for (final file in sorted) {
      final path = file['path'] as String;
      final isDir = file['is_dir'] as bool? ?? false;
      final size = (file['size'] as num? ?? 0).toInt();
      final parts = path.split('/');
      final name = parts.last;

      final node = FileTreeNode(
        name: name,
        fullPath: path,
        isDir: isDir,
        size: size,
      );

      if (isDir) {
        dirMap[path] = node;
      }

      if (parts.length == 1) {
        roots.add(node);
      } else {
        final parentPath = parts.sublist(0, parts.length - 1).join('/');
        final parent = dirMap[parentPath];
        if (parent != null) {
          parent.children.add(node);
        } else {
          // Parent not seen yet (e.g. deeper nesting); add to roots as fallback
          roots.add(node);
        }
      }
    }

    return roots;
  }

  /// Collect fullPaths of all currently expanded directory nodes.
  static Set<String> collectExpanded(List<FileTreeNode> roots) {
    final result = <String>{};
    void visit(FileTreeNode node) {
      if (node.isDir && node.expanded) {
        result.add(node.fullPath);
        for (final child in node.children) {
          visit(child);
        }
      }
    }
    for (final root in roots) {
      visit(root);
    }
    return result;
  }

  /// Re-expand any nodes whose fullPath is in [expandedPaths].
  static void restoreExpanded(List<FileTreeNode> roots, Set<String> expandedPaths) {
    void visit(FileTreeNode node) {
      if (node.isDir && expandedPaths.contains(node.fullPath)) {
        node.expanded = true;
        for (final child in node.children) {
          visit(child);
        }
      }
    }
    for (final root in roots) {
      visit(root);
    }
  }
}
