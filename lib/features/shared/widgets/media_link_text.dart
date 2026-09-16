import 'package:flutter/material.dart';

import '../../core/core_models.dart';

/// Renders a comma-separated list of media items (artists, albums, playlists, ...)
/// where each name is individually tappable and underlines only while hovered,
/// mirroring MAUI's per-item LinkLabelBehavior used in RowView/TableView columns.
class MediaLinkText extends StatefulWidget {
  final List<MediaItem> items;
  final String fallbackText;
  final TextStyle? style;
  final TextAlign textAlign;
  final int maxLines;
  final ValueChanged<MediaItem>? onItemTap;
  final ValueChanged<int>? onItemPointerDown;

  const MediaLinkText({
    super.key,
    required this.items,
    required this.fallbackText,
    this.style,
    this.textAlign = TextAlign.left,
    this.maxLines = 1,
    this.onItemTap,
    this.onItemPointerDown,
  });

  @override
  State<MediaLinkText> createState() => _MediaLinkTextState();
}

class _MediaLinkTextState extends State<MediaLinkText> {
  int? _hoveredIndex;

  void _setHovered(int? index) {
    if (_hoveredIndex == index) {
      return;
    }
    setState(() => _hoveredIndex = index);
  }

  @override
  Widget build(BuildContext context) {
    if (widget.items.isEmpty) {
      return Text(
        widget.fallbackText,
        maxLines: widget.maxLines,
        overflow: TextOverflow.ellipsis,
        textAlign: widget.textAlign,
        style: widget.style,
      );
    }

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final TextStyle baseStyle = widget.style ?? const TextStyle();
        final List<InlineSpan> spans = <InlineSpan>[];
        final BoxConstraints linkConstraints = constraints.maxWidth.isFinite
            ? BoxConstraints(maxWidth: constraints.maxWidth)
            : const BoxConstraints();

        for (int i = 0; i < widget.items.length; i++) {
          final MediaItem item = widget.items[i];
          final bool isTappable =
              widget.onItemTap != null &&
              item.itemId.trim().isNotEmpty &&
              item.provider.trim().isNotEmpty;

          if (isTappable) {
            final bool isHovered = _hoveredIndex == i;
            spans.add(
              WidgetSpan(
                alignment: PlaceholderAlignment.baseline,
                baseline: TextBaseline.alphabetic,
                child: ConstrainedBox(
                  constraints: linkConstraints,
                  child: MouseRegion(
                    cursor: SystemMouseCursors.click,
                    onEnter: (_) => _setHovered(i),
                    onExit: (_) => _setHovered(null),
                    child: Listener(
                      onPointerDown: (PointerDownEvent event) =>
                          widget.onItemPointerDown?.call(event.pointer),
                      child: GestureDetector(
                        onTap: () => widget.onItemTap!(item),
                        child: Text(
                          item.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          softWrap: false,
                          style: baseStyle.copyWith(
                            decoration: isHovered
                                ? TextDecoration.underline
                                : TextDecoration.none,
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            );
          } else {
            spans.add(TextSpan(text: item.name, style: baseStyle));
          }

          if (i < widget.items.length - 1) {
            spans.add(TextSpan(text: ', ', style: baseStyle));
          }
        }

        return Text.rich(
          TextSpan(children: spans),
          maxLines: widget.maxLines,
          overflow: TextOverflow.ellipsis,
          softWrap: false,
          textWidthBasis: TextWidthBasis.parent,
          textAlign: widget.textAlign,
        );
      },
    );
  }
}
