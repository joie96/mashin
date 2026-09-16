import 'package:flutter/widgets.dart';

const double compactLayoutBreakpoint = 900;

bool isCompactLayout(BuildContext context) =>
    MediaQuery.sizeOf(context).width < compactLayoutBreakpoint;
