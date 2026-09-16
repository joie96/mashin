import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'explore_screen_desktop.dart';
import 'explore_screen_mobile.dart';

class ExploreScreen extends StatelessWidget {
  const ExploreScreen({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return ExploreScreenMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
      );
    }

    return ExploreScreenDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
    );
  }
}
