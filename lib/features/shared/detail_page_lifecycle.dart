import 'package:flutter/material.dart';

final detailPageRouteObserver = RouteObserver<ModalRoute<dynamic>>();

/// Keeps read ownership aligned with the visible route, not widget disposal.
mixin DetailPageLifecycle<Page extends StatefulWidget> on State<Page>
    implements RouteAware {
  ModalRoute<dynamic>? _detailRoute;

  void onDetailPageVisibilityChanged(bool isVisible);

  // Route subscription follows navigator changes and is released with the widget.
  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final route = ModalRoute.of(context);
    if (identical(route, _detailRoute)) return;
    detailPageRouteObserver.unsubscribe(this);
    _detailRoute = route;
    if (route != null) detailPageRouteObserver.subscribe(this, route);
  }

  @override
  void dispose() {
    detailPageRouteObserver.unsubscribe(this);
    super.dispose();
  }

  // Covered routes remain mounted, but no longer own active detail requests.
  @override
  void didPush() =>
      onDetailPageVisibilityChanged(_detailRoute?.isCurrent ?? true);

  @override
  void didPushNext() => onDetailPageVisibilityChanged(false);

  @override
  void didPopNext() => onDetailPageVisibilityChanged(true);

  @override
  void didPop() => onDetailPageVisibilityChanged(false);
}
