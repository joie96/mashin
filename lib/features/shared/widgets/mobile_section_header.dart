import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'media_table_section_desktop.dart';

class MobileSectionHeader extends StatelessWidget {
  static const double _separatorGap = 16;
  static const double _playbackActionsWidth = 64;

  final String title;
  final int? itemCount;
  final bool showPlaybackActions;
  final VoidCallback? onPlayTap;
  final VoidCallback? onShuffleTap;
  final String shuffleTooltip;

  const MobileSectionHeader({
    super.key,
    required this.title,
    this.itemCount,
    required this.showPlaybackActions,
    required this.onPlayTap,
    required this.onShuffleTap,
    required this.shuffleTooltip,
  });

  @override
  Widget build(BuildContext context) {
    final TextStyle titleStyle =
        Theme.of(
          context,
        ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700) ??
        DefaultTextStyle.of(context).style;

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final TextPainter titlePainter = TextPainter(
          text: TextSpan(text: title, style: titleStyle),
          textDirection: Directionality.of(context),
          textScaler: MediaQuery.textScalerOf(context),
          maxLines: 1,
        )..layout();
        final double badgeWidth = _badgeWidth(context);
        final double badgeGap = itemCount == null ? 0 : 10;
        final double actionsWidth = showPlaybackActions
            ? _separatorGap + _playbackActionsWidth
            : 0;
        final double titleMaxWidth = math.max(
          0,
          constraints.maxWidth -
              badgeGap -
              badgeWidth -
              _separatorGap -
              actionsWidth,
        );
        final double titleWidth = math.min(titlePainter.width, titleMaxWidth);
        final double separatorWidth = math.max(
          0,
          constraints.maxWidth -
              titleWidth -
              badgeGap -
              badgeWidth -
              _separatorGap -
              actionsWidth,
        );

        return Row(
          children: <Widget>[
            SizedBox(
              width: titleWidth,
              child: Text(
                title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: titleStyle,
              ),
            ),
            if (itemCount != null) ...<Widget>[
              const SizedBox(width: 10),
              SectionCountBadge(count: itemCount!, compact: true),
            ],
            const SizedBox(width: _separatorGap),
            SizedBox(
              width: separatorWidth,
              child: const SectionHeaderDivider(),
            ),
            if (showPlaybackActions) ...<Widget>[
              const SizedBox(width: _separatorGap),
              SectionHeaderIconButton(
                icon: Icons.play_arrow,
                tooltip: 'Abspielen',
                onTap: onPlayTap,
              ),
              const SizedBox(width: 8),
              SectionHeaderIconButton(
                icon: Icons.shuffle,
                tooltip: shuffleTooltip,
                onTap: onShuffleTap,
              ),
            ],
          ],
        );
      },
    );
  }

  double _badgeWidth(BuildContext context) {
    if (itemCount == null) {
      return 0;
    }
    final TextStyle countStyle = DefaultTextStyle.of(
      context,
    ).style.merge(const TextStyle(fontWeight: FontWeight.bold, fontSize: 9.5));
    final TextPainter countPainter = TextPainter(
      text: TextSpan(text: '$itemCount', style: countStyle),
      textDirection: Directionality.of(context),
      textScaler: MediaQuery.textScalerOf(context),
      maxLines: 1,
    )..layout();
    return countPainter.width + 12;
  }
}
