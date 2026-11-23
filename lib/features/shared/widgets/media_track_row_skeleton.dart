import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';

class MediaTrackRowSkeleton extends StatelessWidget {
  const MediaTrackRowSkeleton({
    super.key,
    required this.isFirst,
    this.durationColumnWidth = 34,
    this.providerColumnWidth = 20,
    this.menuColumnWidth = 40,
    this.menuGap = 10,
    this.trailingPadding = 8,
  });

  final bool isFirst;
  final double durationColumnWidth;
  final double providerColumnWidth;
  final double menuColumnWidth;
  final double menuGap;
  final double trailingPadding;

  // Row rendering.
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget skeletonBox({
      required double width,
      required double height,
      double radius = 6,
    }) => Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: colors.bgTertiary,
        borderRadius: BorderRadius.circular(radius),
      ),
    );

    return SizedBox(
      height: 64,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: Colors.transparent,
          border: isFirst
              ? const Border()
              : Border(top: BorderSide(color: colors.border)),
        ),
        child: Padding(
          padding: EdgeInsets.fromLTRB(8, 6, trailingPadding, 6),
          child: Row(
            children: <Widget>[
              Container(
                width: 52,
                height: 52,
                decoration: BoxDecoration(
                  color: colors.bgTertiary,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Icon(
                  Icons.music_note_outlined,
                  color: colors.textSecondary,
                  size: 24,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: <Widget>[
                    skeletonBox(width: 176, height: 15),
                    const SizedBox(height: 4),
                    skeletonBox(width: 124, height: 12),
                  ],
                ),
              ),
              const SizedBox(width: 10),
              SizedBox(
                width: durationColumnWidth,
                child: Align(
                  alignment: Alignment.centerRight,
                  child: skeletonBox(width: 34, height: 12, radius: 4),
                ),
              ),
              const SizedBox(width: 10),
              SizedBox(
                width: providerColumnWidth,
                child: Center(
                  child: skeletonBox(width: 16, height: 16, radius: 8),
                ),
              ),
              SizedBox(width: menuGap),
              SizedBox(
                width: menuColumnWidth,
                height: 40,
                child: Center(
                  child: skeletonBox(width: 18, height: 18, radius: 9),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
