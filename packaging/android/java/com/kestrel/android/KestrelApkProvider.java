package com.kestrel.android;

import android.content.ContentProvider;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import java.io.File;
import java.io.FileNotFoundException;

/**
 * Serves the downloaded Kestrel update APK to the Android system installer.
 * Only one file is ever served: {@code kestrel-update.apk} in the app's
 * cache dir, read-only. The installer reads it through the content URI the
 * launcher hands over, and Android always asks the player to confirm the
 * install; nothing here installs silently.
 */
public final class KestrelApkProvider extends ContentProvider {
    private static final String APK_FILE = "kestrel-update.apk";
    private static final String APK_MIME = "application/vnd.android.package-archive";

    @Override public boolean onCreate() {
        return true;
    }

    private File fileFor(Uri uri) {
        if (getContext() == null) {
            return null;
        }
        String name = uri.getLastPathSegment();
        if (!APK_FILE.equals(name)) {
            return null;
        }
        return new File(getContext().getCacheDir(), name);
    }

    @Override public ParcelFileDescriptor openFile(Uri uri, String mode)
            throws FileNotFoundException {
        if (!"r".equals(mode)) {
            throw new FileNotFoundException("read-only");
        }
        File file = fileFor(uri);
        if (file == null || !file.isFile()) {
            throw new FileNotFoundException(uri.toString());
        }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override public Cursor query(Uri uri, String[] projection, String selection,
            String[] selectionArgs, String sortOrder) {
        File file = fileFor(uri);
        if (file == null || !file.isFile()) {
            return null;
        }
        String[] columns = projection == null
                ? new String[]{"_display_name", "_size"}
                : projection;
        MatrixCursor cursor = new MatrixCursor(columns);
        MatrixCursor.RowBuilder row = cursor.newRow();
        for (String column : columns) {
            if ("_display_name".equals(column)) {
                row.add(file.getName());
            } else if ("_size".equals(column)) {
                row.add(file.length());
            } else {
                row.add(null);
            }
        }
        return cursor;
    }

    @Override public String getType(Uri uri) {
        return fileFor(uri) == null ? null : APK_MIME;
    }

    @Override public Uri insert(Uri uri, android.content.ContentValues values) {
        throw new UnsupportedOperationException("read-only");
    }

    @Override public int update(Uri uri, android.content.ContentValues values,
            String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("read-only");
    }

    @Override public int delete(Uri uri, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("read-only");
    }
}
