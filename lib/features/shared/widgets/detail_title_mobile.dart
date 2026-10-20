import 'package:flutter/material.dart';

import 'detail_provider_switcher.dart';

class MobileDetailTitle extends StatelessWidget {
  const MobileDetailTitle({
    super.key,
    required this.title,
    this.style,
    this.providerSwitcher,
  });

  final String title;
  final TextStyle? style;
  final DetailProviderSwitcher? providerSwitcher;

  // Title and provider layout.
  @override
  Widget build(BuildContext context) {
    final double sideWidth = providerSwitcher == null
        ? 0
        : DetailProviderSwitcher.mobileWidth + 8;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: <Widget>[
          SizedBox(width: sideWidth),
          Flexible(
            child: Text(
              title,
              textAlign: TextAlign.center,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: style ?? Theme.of(context).textTheme.headlineSmall,
            ),
          ),
          SizedBox(
            width: sideWidth,
            child: providerSwitcher == null
                ? null
                : Padding(
                    padding: const EdgeInsets.only(left: 8),
                    child: providerSwitcher,
                  ),
          ),
        ],
      ),
    );
  }
}
