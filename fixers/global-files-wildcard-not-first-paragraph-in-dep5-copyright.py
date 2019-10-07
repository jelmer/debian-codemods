#!/usr/bin/python3


from lintian_brush.copyright import update_copyright, NotMachineReadableError


def swap_files_glob(copyright):
    files_i = 0
    for i, paragraph in enumerate(copyright._Copyright__paragraphs):
        if "Files" in paragraph:
            if paragraph["Files"] == "*" and files_i > 0:
                copyright._Copyright__paragraphs.insert(
                    0, copyright._Copyright__paragraphs.pop(i))
            files_i += 1


try:
    update_copyright(swap_files_glob)
except (FileNotFoundError, NotMachineReadableError):
    pass

print('Make "Files: *" paragraph the first in the copyright file.')
print('Fixed-Lintian-Tags: '
      'global-files-wildcard-not-first-paragraph-in-dep5-copyright')
