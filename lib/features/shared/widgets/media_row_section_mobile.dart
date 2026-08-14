import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import 'media_row_section_desktop.dart';

class MediaRowSectionMobile<T> extends StatelessWidget {
  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final MediaRowCardData Function(T item) cardMapper;
  final double itemWidth;
  final double coverSize;
  final double itemSpacing;
  final int maxVisibleItems;
  final double height;
  final int skeletonCount;

  const MediaRowSectionMobile({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    required this.cardMapper,
    this.itemWidth = 142,
    this.coverSize = 112,
    this.itemSpacing = 8,
    this.maxVisibleItems = 12,
    this.height = 198,
    this.skeletonCount = 6,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          title,
          style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 10),
        SizedBox(
          height: height,
          child: isLoading
              ? ListView.separated(
                  scrollDirection: Axis.horizontal,
                  itemCount: skeletonCount,
                  separatorBuilder: (_, _) => SizedBox(width: itemSpacing),
                  itemBuilder: (_, _) => _MobileRowCardSkeleton(
                    itemWidth: itemWidth,
                    coverSize: coverSize,
                  ),
                )
              : items.isEmpty
                  ? Container(
                      width: double.infinity,
                      padding: const EdgeInsets.all(16),
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(color: colors.border),
                        color: colors.bgSecondary,
                      ),
                      child: Align(
                        alignment: Alignment.centerLeft,
                        child: Text(
                          emptyText,
                          style: TextStyle(color: colors.textSecondary),
                        ),
                      ),
                    )
                  : ListView.separated(
                      scrollDirection: Axis.horizontal,
                      itemCount: items.length > maxVisibleItems ? maxVisibleItems : items.length,
                      separatorBuilder: (_, _) => SizedBox(width: itemSpacing),
                      itemBuilder: (BuildContext context, int index) {
                        final data = cardMapper(items[index]);
                        return _MobileMediaRowCard(
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          icon: data.icon,
                          title: data.title,
                          secondaryText: data.secondaryText,
                          tertiaryText: data.tertiaryText,
                          coverUrl: data.coverUrl,
                          showPlayOverlay: data.showPlayOverlay,
                        );
                      },
                    ),
        ),
      ],
    );
  }
}

class _MobileMediaRowCard extends StatelessWidget {
  final double itemWidth;
  final double coverSize;
  final IconData icon;
  final String title;
  final String? secondaryText;
  final String? tertiaryText;
  final String? coverUrl;
  final bool showPlayOverlay;

  const _MobileMediaRowCard({
    required this.itemWidth,
    required this.coverSize,
    required this.icon,
    required this.title,
    required this.secondaryText,
    required this.tertiaryText,
    required this.coverUrl,
    required this.showPlayOverlay,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasSecondary = secondaryText != null && secondaryText!.trim().isNotEmpty;
    final bool hasTertiary = tertiaryText != null && tertiaryText!.trim().isNotEmpty;

    return SizedBox(
      width: itemWidth,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          Stack(
            children: <Widget>[
              ClipRRect(
                borderRadius: BorderRadius.circular(12),
                child: Container(
                  width: coverSize,
                  height: coverSize,
                  color: colors.bgTertiary,
                  child: coverUrl != null && coverUrl!.trim().isNotEmpty
                      ? Image.network(
                          coverUrl!,
                          fit: BoxFit.cover,
                          errorBuilder: (_, _, _) => Icon(icon, color: colors.accent, size: 28),
                        )
                      : Icon(icon, color: colors.accent, size: 28),
                ),
              ),
              if (showPlayOverlay)
                Positioned(
                  left: 6,
                  bottom: 6,
                  child: Container(
                    width: 32,
                    height: 32,
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surface.withValues(alpha: 0.9),
                      borderRadius: BorderRadius.circular(16),
                      border: Border.all(color: colors.border),
                    ),
                    child: Icon(
                      Icons.play_arrow_rounded,
                      size: 20,
                      color: Theme.of(context).colorScheme.onSurface,
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            title,
            maxLines: 1,
            textAlign: TextAlign.center,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.titleSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
          if (hasSecondary) ...<Widget>[
            const SizedBox(height: 2),
            Text(
              secondaryText!,
              maxLines: 1,
              textAlign: TextAlign.center,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: colors.textSecondary, fontSize: 11),
            ),
          ],
          if (hasTertiary) ...<Widget>[
            const SizedBox(height: 2),
            Text(
              tertiaryText!,
              maxLines: 1,
              textAlign: TextAlign.center,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: colors.textSecondary, fontSize: 11),
            ),
          ],
        ],
      ),
    );
  }
}

class _MobileRowCardSkeleton extends StatelessWidget {
  final double itemWidth;
  final double coverSize;

  const _MobileRowCardSkeleton({
    required this.itemWidth,
    required this.coverSize,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget line(double width) {
      return Container(
        width: width,
        height: 10,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(6),
        ),
      );
    }

    return SizedBox(
      width: itemWidth,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          Container(
            width: coverSize,
            height: coverSize,
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(12),
              color: colors.bgTertiary,
            ),
          ),
          const SizedBox(height: 8),
          line(98),
          const SizedBox(height: 6),
          line(78),
          const SizedBox(height: 6),
          line(66),
        ],
      ),
    );
  }
}
