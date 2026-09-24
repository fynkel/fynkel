// The editor's icon set: Lucide's 16px stroke drawings (blueprint item 9a
// stage d), one constant per icon the UI asks for.
//
// The names are the ones the panels already used — they came from Material
// Symbols, and keeping them means this item's diff shows the drawing
// changing and nothing else. `Icons.` appears nowhere in `lib/` any more:
// a Material glyph beside a Lucide one is the single most obvious way for
// an icon set to look unfinished.
//
// Two of these names are also part of a public contract: `@Expose(icon:
// 'star')` in a game's Dart picks one by its short name (the table in
// `inspector_panel.dart`), so a name here is not free to change.
import 'package:flutter/widgets.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

/// Every icon the editor draws. Sized by the theme (`tokens.icon`, 16).
abstract final class FIcons {
  static const IconData acUnit = LucideIcons.snowflake;
  static const IconData accessibilityNew = LucideIcons.personStanding;
  static const IconData accountTreeOutlined = LucideIcons.listTree;
  static const IconData add = LucideIcons.plus;
  static const IconData addBoxOutlined = LucideIcons.squarePlus;
  static const IconData adjust = LucideIcons.crosshair;
  static const IconData animation = LucideIcons.film;
  static const IconData arrowDownward = LucideIcons.arrowDown;
  static const IconData arrowDropDown = LucideIcons.chevronDown;
  static const IconData arrowForward = LucideIcons.arrowRight;
  static const IconData arrowUpward = LucideIcons.arrowUp;
  static const IconData aspectRatio = LucideIcons.ratio;
  static const IconData audiotrack = LucideIcons.audioLines;
  static const IconData autoAwesomeOutlined = LucideIcons.sparkles;
  static const IconData bolt = LucideIcons.zap;
  static const IconData brokenImageOutlined = LucideIcons.imageOff;
  static const IconData brush = LucideIcons.brush;
  static const IconData build = LucideIcons.wrench;
  static const IconData callSplit = LucideIcons.gitBranch;
  static const IconData cancel = LucideIcons.circleX;
  static const IconData categoryOutlined = LucideIcons.shapes;
  static const IconData centerFocusStrong = LucideIcons.focus;
  static const IconData chat = LucideIcons.messageSquare;
  static const IconData check = LucideIcons.check;
  static const IconData checkCircle = LucideIcons.circleCheck;
  static const IconData chevronRight = LucideIcons.chevronRight;
  static const IconData circle = LucideIcons.circle;
  static const IconData circleOutlined = LucideIcons.circle;
  static const IconData clearAll = LucideIcons.eraser;
  static const IconData close = LucideIcons.x;
  static const IconData code = LucideIcons.code;
  static const IconData copy = LucideIcons.copy;
  static const IconData createNewFolderOutlined = LucideIcons.folderPlus;
  static const IconData cropRotate = LucideIcons.axis3d;
  static const IconData cropSquare = LucideIcons.square;
  static const IconData dashboardCustomizeOutlined = LucideIcons.layoutDashboard;
  static const IconData deleteOutline = LucideIcons.trash2;
  static const IconData descriptionOutlined = LucideIcons.fileText;
  static const IconData directionsRun = LucideIcons.footprints;
  static const IconData doorFrontDoorOutlined = LucideIcons.doorOpen;
  static const IconData eco = LucideIcons.leaf;
  static const IconData editNote = LucideIcons.squarePen;
  static const IconData editShape = LucideIcons.vectorSquare;
  static const IconData error = LucideIcons.circleX;
  static const IconData errorOutline = LucideIcons.circleAlert;
  static const IconData checkCircleOutline = LucideIcons.circleCheck;
  static const IconData expandMore = LucideIcons.chevronDown;
  static const IconData explore = LucideIcons.compass;
  static const IconData extension = LucideIcons.puzzle;
  static const IconData extensionOutlined = LucideIcons.puzzle;
  static const IconData favorite = LucideIcons.heart;
  static const IconData filterList = LucideIcons.listFilter;
  static const IconData flag = LucideIcons.flag;
  static const IconData folderOpen = LucideIcons.folderOpen;
  static const IconData folderOpenOutlined = LucideIcons.folderOpen;
  static const IconData folderOutlined = LucideIcons.folder;
  static const IconData gamepad = LucideIcons.gamepad;
  static const IconData gpsFixed = LucideIcons.crosshair;
  static const IconData grid4x4 = LucideIcons.grid3x3;
  static const IconData gridGoldenratio = LucideIcons.grid2x2;
  static const IconData gridOn = LucideIcons.grid3x3;
  static const IconData healthAndSafety = LucideIcons.heartPulse;
  static const IconData helpOutline = LucideIcons.circleHelp;
  static const IconData homeOutlined = LucideIcons.house;
  static const IconData imageOutlined = LucideIcons.image;
  static const IconData infoOutline = LucideIcons.info;
  static const IconData insertDriveFileOutlined = LucideIcons.file;
  static const IconData inventory2Outlined = LucideIcons.boxes;
  static const IconData key = LucideIcons.keyRound;
  static const IconData landscapeOutlined = LucideIcons.mountain;
  static const IconData lightbulbOutline = LucideIcons.lightbulb;
  static const IconData linkOff = LucideIcons.unlink;
  static const IconData localFireDepartment = LucideIcons.flame;
  static const IconData lockOutline = LucideIcons.lock;
  static const IconData logout = LucideIcons.logOut;
  static const IconData map = LucideIcons.map;
  static const IconData moreHoriz = LucideIcons.ellipsis;
  static const IconData moreVert = LucideIcons.ellipsisVertical;
  static const IconData motionPhotosOnOutlined = LucideIcons.circlePlay;
  static const IconData musicNote = LucideIcons.music;
  static const IconData nearMeOutlined = LucideIcons.navigation;
  static const IconData openInNew = LucideIcons.externalLink;
  static const IconData openWith = LucideIcons.move;
  static const IconData paletteOutlined = LucideIcons.palette;
  static const IconData person = LucideIcons.user;
  static const IconData pets = LucideIcons.pawPrint;
  static const IconData photoCameraFrontOutlined = LucideIcons.camera;
  static const IconData playArrow = LucideIcons.play;
  static const IconData pause = LucideIcons.pause;
  static const IconData psychology = LucideIcons.brain;
  static const IconData public = LucideIcons.globe;
  static const IconData queryStats = LucideIcons.chartNoAxesCombined;
  static const IconData redo = LucideIcons.redo2;
  static const IconData refresh = LucideIcons.refreshCw;
  static const IconData remove = LucideIcons.minus;
  static const IconData rotateLeft = LucideIcons.rotateCcw;
  static const IconData rotateRight = LucideIcons.rotateCw;
  static const IconData science = LucideIcons.flaskConical;
  static const IconData search = LucideIcons.search;
  static const IconData selectAll = LucideIcons.boxSelect;
  static const IconData sensors = LucideIcons.radio;
  static const IconData settings = LucideIcons.settings;
  static const IconData shield = LucideIcons.shield;
  static const IconData shoppingCart = LucideIcons.shoppingCart;
  static const IconData skipForward = LucideIcons.stepForward;
  static const IconData smartToy = LucideIcons.bot;
  static const IconData speed = LucideIcons.gauge;
  static const IconData sportsBasketballOutlined = LucideIcons.circleDot;
  static const IconData sportsEsports = LucideIcons.gamepad2;
  static const IconData star = LucideIcons.star;
  static const IconData stop = LucideIcons.square;
  static const IconData syncProblem = LucideIcons.refreshCwOff;
  static const IconData terminal = LucideIcons.terminal;
  static const IconData textFields = LucideIcons.type;
  static const IconData timer = LucideIcons.timer;
  static const IconData toggleOn = LucideIcons.toggleRight;
  static const IconData touchAppOutlined = LucideIcons.pointer;
  static const IconData tune = LucideIcons.sliders;
  static const IconData unarchiveOutlined = LucideIcons.packageOpen;
  static const IconData undo = LucideIcons.undo2;
  static const IconData upgrade = LucideIcons.circleFadingArrowUp;
  static const IconData upload = LucideIcons.arrowUpFromLine;
  static const IconData videocamOutlined = LucideIcons.video;
  static const IconData videogameAsset = LucideIcons.gamepad2;
  static const IconData viewInArOutlined = LucideIcons.box;
  static const IconData viewListOutlined = LucideIcons.list;
  static const IconData visibility = LucideIcons.eye;
  static const IconData visibilityOffOutlined = LucideIcons.eyeOff;
  static const IconData visibilityOutlined = LucideIcons.eye;
  static const IconData volumeUp = LucideIcons.volume2;
  static const IconData warningAmber = LucideIcons.triangleAlert;
  static const IconData warningAmberRounded = LucideIcons.triangleAlert;
  static const IconData waterDrop = LucideIcons.droplet;
  static const IconData wbSunnyOutlined = LucideIcons.sun;
  static const IconData widgetsOutlined = LucideIcons.package;

  // Item 9b–9d: the Inspector's pages, the picker and the title bar.
  static const IconData pick = LucideIcons.circleDot;
  static const IconData none = LucideIcons.circleSlash;
  static const IconData lock = LucideIcons.lock;
  static const IconData lockOpen = LucideIcons.lockOpen;
  static const IconData layers = LucideIcons.layers;
  static const IconData link = LucideIcons.link;
  static const IconData importSettings = LucideIcons.slidersHorizontal;
  static const IconData chevronDown = LucideIcons.chevronDown;
  static const IconData house = LucideIcons.house;
  static const IconData command = LucideIcons.command;
  static const IconData codegenOk = LucideIcons.circleCheck;
  static const IconData transformTool = LucideIcons.move3d;
  static const IconData rectTool = LucideIcons.squareDashed;
}
