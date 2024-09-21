#!/bin/sh

perl -i -pe \
'BEGIN{undef $/;} s/([ ]+)Free Software Foundation, Inc\., 59 Temple Place - Suite 330,
([ ]+)Boston, MA 02111-1307, USA\./\1Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,
\2MA 02110-1301, USA./smg' debian/copyright

echo "Update FSF postal address."
echo "Fixed-Lintian-Tags: old-fsf-address-in-copyright-file"
