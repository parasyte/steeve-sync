# Steeve Sync

> Good bug!

Synchronize your [Deep Rock Galactic](https://www.deeprockgalactic.com/) saves between the Xbox and Steam editions.

Steeve Sync will silently wait in the background for any changes to the save file in either edition. When it detects a change, it will first make a backup and then copy the new save over the old. The synchronization works in both directions.

Backups can be found in the following directories:

| Edition | Backup files path                                   |
|---------|-----------------------------------------------------|
| Steam   | `%AppData%\KodeWerx\SteeveSync\data\Backups\Steam\` |
| Xbox    | `%AppData%\KodeWerx\SteeveSync\data\Backups\Xbox\`  |

## Limitations

This service will not work properly when multiple DRG accounts are used on the system. Synchronization with multiple Xbox and Steam accounts is well outside of the scope of this tool.

Let's imagine for a moment that the PC has two users: Alice and Bob. Both users have their own Xbox and Steam accounts. Several question arise when attempting to synchronize saves:

1. How does Steeve determine which accounts belong to Alice, and which belong to Bob?
  - Trivially, Xbox account information (including save files) gets stored under each Windows User's profile, which have well-known locations and permissions.
  - However, it is also possible to log in to different Xbox accounts on the same Windows account. In this case, it looks like a similar situation to Steam, where save files for multiple Xbox users are stored in the same directory tree that Steeve watches.
  - Steam account information is usually bundled into the Steam installation directory, with Steam IDs used in the filename. Linking those Steam IDs to a specific Windows User profile is not something that Steeve is currently capable of.
2. Without a mapping strategy for Xbox and Steam accounts, should Steeve try to synchronize to a destination that it cannot guarantee is for the same user?
  - The initial sync implementation will randomly choose a save file to backup and overwrite if there are multiple accounts on the system. It will not attempt to update saves for all accounts, it just picks the first one that it can find.
