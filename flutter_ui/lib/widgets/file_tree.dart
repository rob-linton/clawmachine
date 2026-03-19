import 'package:flutter/material.dart';
import '../models/file_tree_node.dart';

class FileTree extends StatefulWidget {
  final List<FileTreeNode> roots;
  final void Function(String path) onFileTap;
  final void Function(String path, bool isDir) onDelete;
  final void Function(String folderPath) onUploadToFolder;
  final void Function(String? folderPath) onNewFileInFolder;
  final String? selectedFolderPath;
  final void Function(String? folderPath) onFolderSelected;
  final void Function(String path)? onDownload;

  const FileTree({
    super.key,
    required this.roots,
    required this.onFileTap,
    required this.onDelete,
    required this.onUploadToFolder,
    required this.onNewFileInFolder,
    this.selectedFolderPath,
    required this.onFolderSelected,
    this.onDownload,
  });

  @override
  State<FileTree> createState() => _FileTreeState();
}

class _FileTreeState extends State<FileTree> {
  // Flatten the visible nodes (respecting expanded state) into (node, depth) pairs.
  List<(FileTreeNode, int)> _flattenVisible() {
    final result = <(FileTreeNode, int)>[];
    void visit(FileTreeNode node, int depth) {
      result.add((node, depth));
      if (node.isDir && node.expanded) {
        for (final child in node.children) {
          visit(child, depth + 1);
        }
      }
    }
    for (final root in widget.roots) {
      visit(root, 0);
    }
    return result;
  }

  String _formatSize(int bytes) {
    if (bytes < 1024) return '${bytes}B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)}KB';
    return '${(bytes / (1024 * 1024)).toStringAsFixed(1)}MB';
  }

  @override
  Widget build(BuildContext context) {
    final flat = _flattenVisible();
    if (flat.isEmpty) {
      return const Padding(
        padding: EdgeInsets.all(16),
        child: Text('No files', style: TextStyle(color: Colors.grey)),
      );
    }

    return ListView.builder(
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: flat.length,
      itemBuilder: (context, index) {
        final (node, depth) = flat[index];
        return _buildRow(node, depth);
      },
    );
  }

  Widget _buildRow(FileTreeNode node, int depth) {
    final indent = depth * 20.0;

    if (node.isDir) {
      final isSelected = node.fullPath == widget.selectedFolderPath;
      return Semantics(
        label: 'Folder ${node.name}',
        selected: isSelected,
        child: InkWell(
          onTap: () {
            setState(() {
              if (node.expanded && isSelected) {
                node.expanded = false;
                widget.onFolderSelected(null);
              } else {
                node.expanded = true;
                widget.onFolderSelected(node.fullPath);
              }
            });
          },
          child: Container(
            color: isSelected
                ? Theme.of(context).colorScheme.primaryContainer.withValues(alpha: 0.5)
                : null,
            child: Padding(
              padding: EdgeInsets.only(left: indent, right: 4, top: 2, bottom: 2),
              child: Row(
                children: [
                  Icon(
                    node.expanded ? Icons.expand_more : Icons.chevron_right,
                    size: 18,
                    color: Colors.grey[600],
                  ),
                  const SizedBox(width: 2),
                  Icon(
                    node.expanded ? Icons.folder_open : Icons.folder,
                    size: 16,
                    color: Colors.amber[700],
                  ),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      node.name,
                      style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  _folderMenu(node),
                ],
              ),
            ),
          ),
        ),
      );
    } else {
      return Semantics(
        label: 'File ${node.name}',
        child: InkWell(
          onTap: () => widget.onFileTap(node.fullPath),
          child: Padding(
            padding: EdgeInsets.only(left: indent + 22, right: 4, top: 2, bottom: 2),
            child: Row(
              children: [
                const Icon(Icons.insert_drive_file_outlined, size: 16, color: Colors.blueGrey),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    node.name,
                    style: const TextStyle(fontSize: 13),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                Text(
                  _formatSize(node.size),
                  style: const TextStyle(fontSize: 11, color: Colors.grey),
                ),
                _fileMenu(node),
              ],
            ),
          ),
        ),
      );
    }
  }

  Widget _folderMenu(FileTreeNode node) {
    return PopupMenuButton<String>(
      icon: const Icon(Icons.more_horiz, size: 16, color: Colors.grey),
      padding: EdgeInsets.zero,
      itemBuilder: (_) => [
        const PopupMenuItem(value: 'new_file', child: Text('New File Here')),
        const PopupMenuItem(value: 'upload', child: Text('Upload File Here')),
        const PopupMenuItem(
          value: 'delete',
          child: Text('Delete Folder', style: TextStyle(color: Colors.red)),
        ),
      ],
      onSelected: (action) {
        switch (action) {
          case 'new_file':
            widget.onNewFileInFolder(node.fullPath);
          case 'upload':
            widget.onUploadToFolder(node.fullPath);
          case 'delete':
            widget.onDelete(node.fullPath, true);
        }
      },
    );
  }

  Widget _fileMenu(FileTreeNode node) {
    return PopupMenuButton<String>(
      icon: const Icon(Icons.more_horiz, size: 16, color: Colors.grey),
      padding: EdgeInsets.zero,
      itemBuilder: (_) => [
        if (widget.onDownload != null)
          const PopupMenuItem(
            value: 'download',
            child: Text('Download'),
          ),
        const PopupMenuItem(
          value: 'delete',
          child: Text('Delete File', style: TextStyle(color: Colors.red)),
        ),
      ],
      onSelected: (action) {
        if (action == 'download') widget.onDownload?.call(node.fullPath);
        if (action == 'delete') widget.onDelete(node.fullPath, false);
      },
    );
  }
}
