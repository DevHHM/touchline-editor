import hashlib, json, os, pathlib, stat, subprocess, zipfile
root = pathlib.Path(os.environ["GITHUB_WORKSPACE"]).resolve(strict=True)
archive = root / "Touchline-Editor-0.12.0-source.zip"
expected_archive = "2e7f612c9ecf1185eead72085cd4a624e616d354fcc037fd45f22ede13972247"
expected = json.loads(r'''{
  ".gitignore": "0e3ab7f302eceffadd4c20f24656928cba876de42de452719c1d24ea8b7b7c38",
  "AGENTS.md": "d232114e8ce51883d74863a655173a9dcc36be9654df21e2b8d5e370301c5813",
  "BUILDING.md": "f6c8d3ab402934c92569a23f6b4920138ece8798fa3d5756f908b4aea47c85ab",
  "Cargo.lock": "03c0315b0eba21c83dfd99c272036177e64228d6ad684b1dbebb31b597068acf",
  "Cargo.toml": "4905281622ea0e3dff539c78dbe0a7c292372995567b90479b3ebf9c37d90c61",
  "README.md": "fb18ae3193400bba67ede0c29faecc58a1c804fcd378c2632b963f38fe906dcc",
  "THIRD-PARTY-NOTICES.md": "b21f7cdbdd8fd63d307f05aed4070cd05d733f2602bee30be9f5a8b3bc35416e",
  "assets/fonts/OFL-Pretendard.txt": "d31ddd9f2bed32fd7e302a205cf2380ba0de6529152d239ef99cfb6f261bfc04",
  "assets/fonts/Pretendard-Bold.otf": "2e91915fab54df71cc9598ebf608b2bdb54c6fe3c066ac61dff0bc44fca71cc7",
  "assets/fonts/Pretendard-Medium.otf": "d39e50e4bb52b4993b6a4eeb821a171254745bd824446af01e1f616b89fface0",
  "assets/fonts/Pretendard-Regular.otf": "3ffbacde6ab8411f1d2db54bb9b1f0b3ee2a738932033722cf0388c06aed1c93",
  "assets/fonts/Pretendard-SemiBold.otf": "c89bc43027dc7cde5726e96223376f8eec09302b2fc1f8147fd5b57cfc376118",
  "assets/fonts/SOURCES.txt": "75aca0628950e4dbb498af1886adb026d2b405041dea2e1489991cb394e43259",
  "assets/icons/generation.json": "e2cde7ae86d0b4f2c18696ae2f46d175933e1ee8bddea1cb612a3f88c5e8f4a0",
  "assets/icons/touchline.ico": "fc0a1a7763e600a59021e51fb1fa764d6e699d338a70baf337082bef0b8aa7e2",
  "assets/icons/touchline.png": "c31c09733ce089380630ec63e186be1a03f729f6de522f62c5e269c4ca12fef3",
  "assets/licenses/README.txt": "6b86d6d27103a45fe75a8229ca06f51b49ddadde704df086b4d7aa1328792166",
  "assets/licenses/clipboard-win-5.4.3-LICENSE.txt": "c9bff75738922193e67fa726fa225535870d2aa1059f91452c411736284ad566",
  "assets/licenses/egui-0.33.0-LICENSE-APACHE.txt": "8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90",
  "assets/licenses/egui-0.33.0-LICENSE-MIT.txt": "95ca92f5f8ea5231f1580b3a2a799e8260af3114b900e1def5355a7f44bcf60c",
  "assets/licenses/egui-0.33.3-LICENSE-APACHE.txt": "8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90",
  "assets/licenses/egui-0.33.3-LICENSE-MIT.txt": "95ca92f5f8ea5231f1580b3a2a799e8260af3114b900e1def5355a7f44bcf60c",
  "assets/licenses/enum-map-Apache-2.0.txt": "074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff",
  "assets/licenses/enum-map-MIT.txt": "b85dcd3e453d05982552c52b5fc9e0bdd6d23c6f8e844b984a88af32570b0cc0",
  "assets/licenses/enum-map-dep5.txt": "9f7e393571a2a7dd11485db90d8ea9a3189cd8c21ee949f4b487836d4450efc9",
  "assets/licenses/epaint-default-fonts-0.33.3-Hack-Regular.txt": "47c0cccbeec7e8614548cc485588b28149e7874188df5f41b36efebcee285c87",
  "assets/licenses/epaint-default-fonts-0.33.3-OFL.txt": "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
  "assets/licenses/epaint-default-fonts-0.33.3-UFL.txt": "2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002",
  "assets/licenses/epaint-default-fonts-0.33.3-emoji-icon-font-mit-license.txt": "b9d2c1d909aa149996fd4c91dcb92b2362a04431640c1d200959da94caf8cde1",
  "assets/licenses/gl-rs-LICENSE.txt": "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
  "assets/licenses/profiling-1.0.18-LICENSE-APACHE.txt": "10d30a673cd5e9349bdc02aeb48f14b3386d27d0da32df8f0a555d4aa16aa551",
  "assets/licenses/profiling-1.0.18-LICENSE-MIT.txt": "c8167fdeeed46d3f244d3f85c5bf998ce889343691c32be2c61a8bc4b5c08333",
  "assets/licenses/sources.json": "1854b3dd3e050f4232f69c5bb85997dcfe4317be92eb36209cf3d830b6ff03c8",
  "build-api.ps1": "2720809659d58f0742d31cfe4bac9edac7865bf81e1ad7b0c39c1106073e4775",
  "build-release.ps1": "44745f1f9d0b7add1303e982b5a834c359f44c57fe40cb3920fd1013d8f0ef62",
  "build-wpf.ps1": "b223fe6b70cb33a031d629b23937df494e9cec4493b930e26dfc4f9e0ed91951",
  "desktop/Touchline.ClientTests/ClubGrowthAvailabilityTests.cs": "7ec925d6b313de733dca6f2858666b45b8de0f081b3c44ef12232e97af973e61",
  "desktop/Touchline.ClientTests/DesktopUsabilityTests.cs": "74484dc9f1d369fab7334e279637768d456e9a714a7399276ae05a0606c78099",
  "desktop/Touchline.ClientTests/DetailPresentationTests.cs": "dfd70ebdd623cfdc762618f3f97ec782217031843965da1938872be4177cf53c",
  "desktop/Touchline.ClientTests/GameEditionTests.cs": "45c532e86e3d95d2551537708737004d29969de1a046e126a865e5c31b8a00c5",
  "desktop/Touchline.ClientTests/LiveReloadTests.cs": "953cffc279e2e73bfd0e7d6d73fe337bdfc918583e3b126357628bfd46aa355d",
  "desktop/Touchline.ClientTests/PackagePathsTests.cs": "0f8f5da3a06a080deac28f73faf6968b7f7863b36ea063707824074a08992261",
  "desktop/Touchline.ClientTests/PositionFilterTests.cs": "c81848a0a24a8ca7516be34a31755ffe5a98735c93f7417b10154cdb180685d4",
  "desktop/Touchline.ClientTests/Program.cs": "315df0a6edcc27b642a827fefb70b3c645d16558624d167de755704ba13833aa",
  "desktop/Touchline.ClientTests/SearchColumnTests.cs": "b8309fc49ae1262ea62fea673e14d09514b7263d71a7aec292f795ed62584303",
  "desktop/Touchline.ClientTests/Touchline.ClientTests.csproj": "d759c220775e992e62f92abc95120786d079e78d8b91b28ce889f5163948bfbc",
  "desktop/Touchline.Wpf/App.xaml": "728940b0e8205694cf9d22b91d4c99f8fa11a1d7087cc240206e7a57fcfa5535",
  "desktop/Touchline.Wpf/App.xaml.cs": "adfdd47bfa6d4911e9616e638d7efbec942b831a4be4d36e3cd18bd5197486d5",
  "desktop/Touchline.Wpf/AppDialog.cs": "10692c92e229743b51a9bfe6c9b63025e843620200e0fc9fd7705c0c080dc958",
  "desktop/Touchline.Wpf/BridgeClient.cs": "fba370c1380a88db557df716e7a1769aeb537f696c2a4aa8d08c02aa278a6546",
  "desktop/Touchline.Wpf/CareerTargetPicker.cs": "0e0d1c7514c88e756d55fe44fc3a1add036d3520598a6720d615f6390ecaec75",
  "desktop/Touchline.Wpf/ClubGrowthAvailability.cs": "9470b2084ccf3d1466f6648804a4d4f0cfb0f63f229b1dca6b0a5bf6c4bf982d",
  "desktop/Touchline.Wpf/ClubRosterView.cs": "be981421801f4219336234a98f596b9c861e14efb8b56701ac8e31f36ff43cb4",
  "desktop/Touchline.Wpf/ClubStaffReviewModels.cs": "f39dcd3215dfa57128fc6bf6b229df9defb7c56f2bc099068c80593c7e3c13ba",
  "desktop/Touchline.Wpf/ClubStaffReviewView.cs": "0509d5766c9c88c2b73a66b31d2614ae64a2c1d903d594bacbf03a8969f9aafb",
  "desktop/Touchline.Wpf/CoachOverlayWindow.cs": "b33679e4666e3def2ab31717b18cd6459e6fd94dc8487ec86623c0c8b3faada1",
  "desktop/Touchline.Wpf/DesktopWindowPreferences.cs": "d805cd894f13e823eb0faea6deba2aedbd984bc343818c6396ad04b94200e7f5",
  "desktop/Touchline.Wpf/DetailPresentation.cs": "4b2f58dff8825f5e875b61844e5872b1fb24be962c59e23cf66d1de9b0a40aed",
  "desktop/Touchline.Wpf/EditorShortcuts.cs": "a5e157bbb214bf49e011cdeba67f89661fe60d9611d96025d32485f860414ce7",
  "desktop/Touchline.Wpf/EntityRoleSorting.cs": "4b6b6c1d9556ab5e7abd4763e29320546a657e1907096c6f0d8c787359388b3d",
  "desktop/Touchline.Wpf/FieldConverters.cs": "3299fa0ca3d377537f9b484af61d4cfa8b76c1966f3c55ee981fc0654a79c438",
  "desktop/Touchline.Wpf/GameEditionPresentation.cs": "9ec581baafa55016183fa6107aac5518f4f2bacbb0813873e9fd4c19be410fdd",
  "desktop/Touchline.Wpf/LayoutHelpers.cs": "573c89b934c266debe4fb2888ace3cd5299d174ec341b5ddbdd6297e3b7b1c2f",
  "desktop/Touchline.Wpf/LiveReloadState.cs": "cc949781881de7b2ed01e6f0ea18c8b6f889a33dd96502611f9400181f135da3",
  "desktop/Touchline.Wpf/MainWindow.Layout.cs": "a037c3aeb6772ce6c70b2202a003f7d17ac769f8c60cbce1cc2f8ee0e974f277",
  "desktop/Touchline.Wpf/MainWindow.LiveReload.cs": "7dd97d473ee5ae559f8dac1b0cf19576934f0efc6f19020b25bf7d3bc3f8c6a6",
  "desktop/Touchline.Wpf/MainWindow.Relations.cs": "56482e1d20881ffeede2fe8fff941819e153aae12fb2f79ddafccf75fbe8f7b2",
  "desktop/Touchline.Wpf/MainWindow.Roster.cs": "bc722b2aae8ad6c1155edc93bbf0a07ed21f5892593941c6fe9a06fc79aa9ffd",
  "desktop/Touchline.Wpf/MainWindow.Search.cs": "1c6eff7c3a3f35c7f99915a3cf22df14a391145daa4cb15ab2bf0436094931e8",
  "desktop/Touchline.Wpf/MainWindow.StaffReview.cs": "12add64da40c52b1a12b63554f95deaf6c1b6e7d50dd1f64e312ca0b2e5dfb05",
  "desktop/Touchline.Wpf/MainWindow.Usability.cs": "05e34d5b7ebb207ca9ea9204873a302cfcb73d633a5aea0a6617554b3b096bd4",
  "desktop/Touchline.Wpf/MainWindow.Workbench.cs": "d6e22b0a4f5e102925ac5186f09d86ca41a18ad4032de3f1d82682518f6730b2",
  "desktop/Touchline.Wpf/MainWindow.xaml": "fcb23586273fa698c39e71b9ebcc015f1c60032704ce76f4e084eb98fc469571",
  "desktop/Touchline.Wpf/MainWindow.xaml.cs": "50ce3d191d87fa9570b2b55428f9c62c1b9d53a942d7ecfe92828113ca5fa21e",
  "desktop/Touchline.Wpf/Models.cs": "4629e679d14670bd644e91b720d8f92c26d8b109368f3894f63d8b1cde70878f",
  "desktop/Touchline.Wpf/NameCopy.cs": "ed3e5ed67ee7bd1778cfaf294db89bfcac0602229bc22920fa873f9539590d81",
  "desktop/Touchline.Wpf/NameLocalizationModels.cs": "ef591fd6e9f8ea92c22d439827cf11d27f2c5900ed6517d745b9a5ef1d282240",
  "desktop/Touchline.Wpf/NameLocalizationWindow.cs": "8d9f5d45b18e5a98c71cf49f1d9cd79fa32a607b6a07087393eb4e82a4cd2cc0",
  "desktop/Touchline.Wpf/PackagePaths.cs": "73cf234a438f2542893768b71c38486659177e46e4c88d5066b538084a213689",
  "desktop/Touchline.Wpf/PersonRelationsModels.cs": "be360e1f6ea0cd0f987f7d623f7efb321e3371cfd8f6b2b2b6aca4b1430f9a39",
  "desktop/Touchline.Wpf/PersonRelationsView.cs": "9358bbcd8304ab3fffe213acf43d9b12192865531e2f47c4a62d96338b927e68",
  "desktop/Touchline.Wpf/PositionOrder.cs": "19b04e1ecc0cef8b792fb4cfdb78e3c0345491511e45ffb37f372ccebe1de634",
  "desktop/Touchline.Wpf/SearchColumnWindow.cs": "6db737a0b80235743712c3e5fbbea046de30dd1e3be50f57896cc730667dc28c",
  "desktop/Touchline.Wpf/SearchColumns.cs": "4942863834650cbb83e1b871c7845828b97d588e8eb38a3518e9bf5590235585",
  "desktop/Touchline.Wpf/SearchCriteria.cs": "cb88d221ca395a746b3bab8d9ab92afe363a6cb03532495c027ae379b4c87fa5",
  "desktop/Touchline.Wpf/SearchCriteriaWindow.cs": "c1dbde17536a2731ad5ef11735899bd334bd837d803b85194b1cbf3007777915",
  "desktop/Touchline.Wpf/SearchView.Columns.Qa.cs": "b2131b047db148ed86ba48202e38b6a5d1dc61ee27bdd861afb9f2c995db8042",
  "desktop/Touchline.Wpf/SearchView.Columns.cs": "ac679a0de98a48ba7ce9f8c285e389df545fe115fcfe48a5b7ef3c1828b7aae7",
  "desktop/Touchline.Wpf/SearchView.cs": "54e592338bdb1c657a65cc9a6abb4d2d832d7576efb66d6854fe9544ee6ffafa",
  "desktop/Touchline.Wpf/SquadGroups.cs": "b0ddaaec65bc025c2ca0c71651c1c6dd8e283e4faa41057a0e8b529dfed66e30",
  "desktop/Touchline.Wpf/ToolsWindow.cs": "4dd2c613d7260fda2b1ea9a53797f9b4111247e9918c583ff0fb9ffbfcc41eee",
  "desktop/Touchline.Wpf/Touchline.Wpf.csproj": "7a715644a37a1e12b7f4e904b79bd525175db673792b41e4884673a4279b614a",
  "desktop/Touchline.Wpf/WorkbenchModels.cs": "2dd8fd060ce80a60ed1a34c7725540a2e18d557087b2b8415ea52783aec0629e",
  "desktop/Touchline.Wpf/WorkbenchWindow.cs": "473c005de9660be50088645d8f01e0ba0e293cbdc712301e0867185c9509b0e4",
  "desktop/Touchline.Wpf/app.manifest": "6ac8f812764efbabb4908a8adf045c14b1347731c552a04f2718847174b526c3",
  "desktop/Touchline.Wpf/packages.lock.json": "4ca706f4ad68fc7cc51125a1c2ad0476aca96c6499919a3d1babea4d8ccd1bf2",
  "distribution/QUICKSTART.ko.md": "6d5d00869cf4ee83aa0808bc809104441b9355df39c8cda7cd5034deac99b8a9",
  "distribution/README.txt": "c136b54da18ac00c2bbbce3852b8385de6b5b58525f7a9082c7996d6d236002c",
  "distribution/SUPPORT.ko.md": "f55bd59675a7035979aef3747d96706b5e508457122055fdd6a57fca9569a612",
  "distribution/VERIFICATION.ko.md": "ff054f5c3d4bf8cef676d66716462da2cbd47ca36f1dbbbb65f2388449cf04f9",
  "distribution/Verify-Package.ps1": "4191ef6abb40a4bbdd0c06f92d42f904c6234172ab92ec37be25a409ca9dd751",
  "distribution/start-api.ps1": "45e4edc275f5d9bc13098bc2675c1ea97262d33acd2fc36dbb496eea91934c4d",
  "reference/local-api-operations.json": "8b8f966c2f18b07901ff1c03246982a266eded3595caad43d9450f0255a05c7c",
  "reference/local-api.md": "a8a4f4084a4da7abbb9b3da7315b52c65dd81fef9de6fa40ab9c07f8ba33ba28",
  "src/api.rs": "6986f60a4d2cfb9ddb7c55a5fff576b93d806f3ac448594c8fb99fc5c4b3e65b",
  "src/assets.rs": "e40beb1bd1ae916603742894fde5ed8bbfe03e6a504da754370fa82d847c01a1",
  "src/backend.rs": "3a947be8c1aeb440d2436a6f8a22750745798e960587469c2461ad1e20749d53",
  "src/backend/profiles.rs": "884ac4193f76c9af602cf6b938eb43472fd28fb58e24b2ef7cc03a8b4f91be36",
  "src/backend/watch.rs": "cee895d6d03e87d98f780a47d224701322c3bfdf2888462a3799ab5d33b44a1f",
  "src/batch.rs": "227375bb04ae0af173bd71231c0780133febbc7721142582cccde332f1154876",
  "src/bridge.rs": "9eb1c8d7826e5802c09d80b9d52680c5a5791e53f1b93a806de672806f6abfb5",
  "src/bridge/api.rs": "45eadfbcbc32a3f8d2f78c6544179f4b8c2fc8dcef90de803fb4c0dec2df403e",
  "src/bridge/connection.rs": "143eaf31b8d616379c91c8cec30f0f418861daa39edd509ef860819a5b48cfc7",
  "src/bridge/name_localization.rs": "ce84ff7d2e8bf1b27e0c0d3b5e8828085e503df2df4ba4083dae1650761ed9e5",
  "src/bridge/relations.rs": "00b83ef8a104fbe3a924408ea23c33a575395402c0a46b0f2933ed4564e8cbbe",
  "src/bridge/staff_review.rs": "188a662e9fdf71e7482e879c363e2c980000f390e60f17cdead70b006c309c75",
  "src/bridge/workspace.rs": "fe3e87b19a4c7d58bd5424009309267523a3834381e8bff771c8c3cc62084ba5",
  "src/compatibility.rs": "b92bf5b0c7185964a85d2deaf80a17ac28298eef4609c1af1fb3db2ea252207f",
  "src/diagnostics.rs": "d705aeaae57779e22e03925e805efe50e5c500b2a613ba56ade0dc5e611740ed",
  "src/event_ui.rs": "3d5605999d20d26db96269654d5e34ae3e5604c322b0bdeab7c4d449a0023c7c",
  "src/events.rs": "b08f22e5d0de8c0d22d978412d987a0b39efe4ebab55854b346c89dbd674b4f2",
  "src/extended.rs": "3d64d06a665169f69839d04ac13a44f302008812fcf0a8d0208124278c6152c4",
  "src/extras.rs": "8e3aa82b3eb9a85e33a7d55059d9a61ff749d2fd6fbe5f65c9c44fd0ec2ddfcf",
  "src/extras/cities.rs": "e05c5d43461cd995c347bd4f08d8fdfa2d5b4050de330171a1c9dcfd7e3f79a3",
  "src/fields.rs": "f3f783ac81a0784d8b56b5d2ba6c1072419824b0d460cd8ae3138ecc6d54d419",
  "src/growth.rs": "fc3812cf1ee073697194460b412e9ee0fd4ed27f881192dace5efdb2b267b6e6",
  "src/main.rs": "b5a720a5ac6324ae87fc06dc9520293d2acdbaa07f88d8a647c616c47c0479e9",
  "src/memory.rs": "78215ee8ba40cf787131b239dda1ab893317608dd45bf67262d47605f69ee92c",
  "src/memory/quiescence.rs": "15962c3a9e5c5932fd99360b18b0f3e2e18e6acdd28bfd94fdfba4be88ca78e9",
  "src/model.rs": "8f684ba07ab9de0bb6ed52fef04c902c873f5e81fe9b6300efb8524f90279ccb",
  "src/name_localization.rs": "4c7da904f8e7ba6574ba5386f0cdefedade8ec01cdfe3a523fe092f226502fbf",
  "src/native_alloc.rs": "a6a1f30b18fa86dcbe7df58e311db5fec90b07b00fdf59a5766e23e6b4cf60a2",
  "src/navigation.rs": "883ef302b651defba9021cbe246faf32d0bbd929d5ea213b094ba814c835221a",
  "src/overlay.rs": "da599e7034a52ab78b45a89a7a0d3ddec876b312e50c104774d1f0e057f2f5ae",
  "src/overlay_context.rs": "a7badce5e244db01168ba17436b3b34142477d6274409b8808587421d6ee5c11",
  "src/relations.rs": "37b43a98fb86dea1734332424e9c4c2e039f7df233e65f297cfc04e669126c91",
  "src/relations/career_growth.rs": "c7a503fc69b597b779c0eb1cea76c059cc0701362497a9299c395587953b9752",
  "src/relations/fixtures.rs": "319f26192b0d9117af4e5863366d4b9a66e0868a52fdda235c139490ed108c6d",
  "src/relations/growth.rs": "d35fc29a01f74a5abd329b796afe82825d791701639df2aa914531ddd73cfb10",
  "src/relations/social.rs": "0a8aefd915c7600e652981b2f450186cc8bb23710442c89d97f13794230f1782",
  "src/relations/targets.rs": "e2eedba8bfeeb22813dfd5558e94b938a0ddfe0585c50160d6015f54a7b39cb3",
  "src/staff_review.rs": "a11275746f9d51c94339a47b997b18322ceb5972357e531966e2cd871a9b2b4b",
  "src/traits.rs": "b55e2d1e9c7598d47538ce64585d74e1e79a9c536a439dfa0bb161ac9dbd1c87",
  "src/transaction.rs": "2e72e0ab86e2aa0a26048b23a45ffa0c1345626dce5296df5c83d10b2e4b18aa",
  "src/typography.rs": "eed7d218f236c22a3c81492757fdc611472018a62fdf3c54e31edccd4a547ce4",
  "src/ui.rs": "e5bb55e94cd4186d14824cc7463b0dbf5f38d2492a28efbb314bc489ab79adbc",
  "src/workspace.rs": "e63872f1da2db05061fdd55183684e88fdbc71ab48ff77c8af8f7e12a353db43",
  "tools/Copy-Supplemental-Notices.ps1": "9af82b7ee086823e1b7b49da7b503f7aab2e3702ef23fa09d67df77b68a90cfc",
  "tools/Get-PeImports.ps1": "76a5323eb2d7378cfff527ff1b65e5220cfb90bb442120682b95a86d60868280",
  "tools/build-launcher.ps1": "2e6df4ca3e56cb788ec61e8343e02f8d48c587e7eb8cdaf5721bcd0c25ff8044",
  "tools/launcher.rs": "f232a3eb9fb535386512f62165231b5e7ae73fc84d763c0489d1627a6115018d",
  "tools/package-app-icon.ps1": "d647dbf63c243f09f5e44d25c76a3204c49ecd5814f5a84ae0b8a8ebcd022cf8",
  "tools/package-wpf.ps1": "d84b5adb6fed81014efe16a8fa02d8811e425f775eeadd461b9b1528ed63a391",
  "tools/start-api.ps1": "cb6bc88687a083fcf7f556a404704510055b0f972f14ab161f4314231731df95",
  "tools/test_touchline_api.py": "fce07c0c89d75ac577662e27d911d0ec6e32fb05207454ff20c7ef749bd2de0d",
  "tools/touchline_api.py": "893a6e1a86a69af6d11968fd36fe3c89cd5a712933658076ff2a62315aa43dc9"
}''')
assert hashlib.sha256(archive.read_bytes()).hexdigest() == expected_archive, "Source archive checksum mismatch"
# One-time import: never overwrite an already imported or independently created source tree.
assert not (root / "src/main.rs").exists(), "Source tree is already present; bootstrap cannot run twice"
assert not (root / "Cargo.toml").exists(), "Source project is already present"
with zipfile.ZipFile(archive) as z:
    infos=z.infolist()
    assert len(infos)==len(expected) and {i.filename for i in infos}==set(expected), "Unexpected archive members"
    assert sum(i.file_size for i in infos)<32*1024*1024, "Unexpected total expanded size"
    validated=[]
    for info in infos:
        name=info.filename
        path=pathlib.PurePosixPath(name)
        mode=info.external_attr>>16
        assert not info.is_dir() and not path.is_absolute() and "\\" not in name and "\x00" not in name
        assert str(path)==name and all(p not in {"", ".", "..", ".git", ".github"} for p in path.parts)
        assert not stat.S_ISLNK(mode) and (stat.S_IFMT(mode) in {0, stat.S_IFREG})
        assert info.file_size<25*1024*1024
        raw=z.read(info)
        assert hashlib.sha256(raw).hexdigest()==expected[name], "Member checksum mismatch: "+name
        destination=root.joinpath(*path.parts)
        assert destination.resolve().is_relative_to(root)
        assert not any(p.is_symlink() for p in [destination,*destination.parents] if p!=root)
        if destination.exists() and name not in {"README.md", "BUILDING.md", "THIRD-PARTY-NOTICES.md"}:
            assert destination.is_file() and destination.read_bytes()==raw, "Existing file conflicts: "+name
        validated.append((destination,raw))
    # Nothing from the archive is imported or executed. Only checked bytes are written.
    for destination,raw in validated:
        destination.parent.mkdir(parents=True,exist_ok=True)
        destination.write_bytes(raw)
for name,digest in expected.items():
    assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest
def git(*args):
    return subprocess.run(["git",*args],cwd=root,check=True)
git("config","user.name","github-actions[bot]")
git("config","user.email","41898282+github-actions[bot]@users.noreply.github.com")
git("add","--",*sorted(expected))
git("-c","core.hooksPath=/dev/null","commit","-m","Import reviewed Touchline Editor 0.12.0 source")
git("push","origin","HEAD:main")
print("Imported and committed",len(expected),"reviewed source files.")
