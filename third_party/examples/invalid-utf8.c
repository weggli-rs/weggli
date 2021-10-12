// ©

void MyBuggyFunction( void* data )
{
	char buf[10];
	memcpy( buf, data, 20 );
}
