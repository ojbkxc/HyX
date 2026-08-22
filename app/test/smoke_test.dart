import 'package:flutter_test/flutter_test.dart';
import 'package:hyx_app/util/formatters.dart';

void main() {
  test('formatBytes handles units', () {
    expect(formatBytes(0), '0 B');
    expect(formatBytes(1023), '1023 B');
    expect(formatBytes(1024), '1.0 KB');
    expect(formatBytes(3 * 1024 * 1024), '3.0 MB');
    expect(formatBytes(2 * 1024 * 1024 * 1024 * 1024), '2.0 TB');
  });

  test('formatSpeed and formatDuration', () {
    expect(formatSpeed(0), '—');
    expect(formatSpeed(12.4 * 1024 * 1024), '12.4 MB/s');
    expect(formatDuration(const Duration(seconds: 90)), '01:30');
    expect(formatDuration(const Duration(hours: 1, minutes: 5)), '1:05:00');
  });

  test('etaMillis returns 0 without speed', () {
    expect(etaMillis(100, 1000, 0), 0);
    expect(etaMillis(100, 1000, 900), 1000);
  });

  test('mimeTypeOf maps common extensions', () {
    expect(mimeTypeOf('a.mp4'), 'video/mp4');
    expect(mimeTypeOf('a.PNG'), 'image/png');
    expect(mimeTypeOf('a.pdf'), 'application/pdf');
    expect(mimeTypeOf('a.apk'), 'application/vnd.android.package-archive');
    expect(mimeTypeOf('noext'), 'application/octet-stream');
  });
}